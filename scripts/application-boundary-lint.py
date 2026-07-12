#!/usr/bin/env python3
"""Enforce presentation-neutral Rust dependencies under src/application."""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


COMMAND_MODULES = {"commands", "tui"}
PRESENTATION_FRAMEWORKS = {
    "colored",
    "console",
    "crossterm",
    "dialoguer",
    "gpui",
    "ratatui",
}
TERMINAL_IO = {"stdin", "stdout", "stderr", "IsTerminal"}
OUTPUT_MACROS = {"print", "println", "eprint", "eprintln", "dbg"}


class ScanError(Exception):
    """Raised when source cannot be scanned safely."""


@dataclass(frozen=True)
class Token:
    value: str
    line: int
    column: int


@dataclass(frozen=True)
class Violation:
    label: str
    token: Token


@dataclass(frozen=True)
class ImportPath:
    path: list[Token]
    alias: Token | None


@dataclass
class Scope:
    aliases: dict[str, list[Token]]
    symbols: set[str]

    @classmethod
    def empty(cls) -> Scope:
        return cls({}, set())

    def bind_alias(self, name: str, path: list[Token]) -> None:
        self.symbols.discard(name)
        self.aliases[name] = path

    def bind_symbol(self, name: str) -> None:
        self.aliases.pop(name, None)
        self.symbols.add(name)


def blank_range(characters: list[str], start: int, end: int) -> None:
    for index in range(start, end):
        if characters[index] != "\n":
            characters[index] = " "


def raw_literal_end(source: str, start: int) -> int | None:
    prefix_length = 0
    if source.startswith(("br", "cr"), start):
        prefix_length = 2
    elif source.startswith("r", start):
        prefix_length = 1
    else:
        return None

    cursor = start + prefix_length
    hash_count = 0
    while cursor < len(source) and source[cursor] == "#":
        hash_count += 1
        cursor += 1
    if cursor >= len(source) or source[cursor] != '"':
        return None

    delimiter = '"' + ("#" * hash_count)
    closing = source.find(delimiter, cursor + 1)
    if closing < 0:
        raise ScanError("unterminated raw string literal")
    return closing + len(delimiter)


def quoted_literal_end(source: str, quote: int) -> int:
    cursor = quote + 1
    while cursor < len(source):
        if source[cursor] == "\\":
            cursor += 2
            continue
        if source[cursor] == '"':
            return cursor + 1
        cursor += 1
    raise ScanError("unterminated string literal")


def char_literal_end(source: str, quote: int) -> int | None:
    value = quote + 1
    if value >= len(source) or source[value] in {"'", "\n", "\r"}:
        return None

    if source[value] != "\\":
        closing = value + 1
        if closing < len(source) and source[closing] == "'":
            return closing + 1
        return None

    escape = value + 1
    if escape >= len(source):
        raise ScanError("unterminated character literal")
    if source[escape] == "x":
        closing = escape + 3
    elif source[escape] == "u" and escape + 1 < len(source) and source[escape + 1] == "{":
        brace = source.find("}", escape + 2)
        if brace < 0:
            raise ScanError("unterminated Unicode character escape")
        closing = brace + 1
    else:
        closing = escape + 1

    if closing < len(source) and source[closing] == "'":
        return closing + 1
    raise ScanError("unterminated character literal")


def code_without_literals(source: str) -> str:
    characters = list(source)
    cursor = 0
    while cursor < len(source):
        if source.startswith("//", cursor):
            end = source.find("\n", cursor + 2)
            if end < 0:
                end = len(source)
            blank_range(characters, cursor, end)
            cursor = end
            continue

        if source.startswith("/*", cursor):
            depth = 1
            end = cursor + 2
            while end < len(source) and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                raise ScanError("unterminated block comment")
            blank_range(characters, cursor, end)
            cursor = end
            continue

        raw_end = raw_literal_end(source, cursor)
        if raw_end is not None:
            blank_range(characters, cursor, raw_end)
            cursor = raw_end
            continue

        if source.startswith(("b\"", "c\""), cursor):
            end = quoted_literal_end(source, cursor + 1)
            blank_range(characters, cursor, end)
            cursor = end
            continue

        if source[cursor] == '"':
            end = quoted_literal_end(source, cursor)
            blank_range(characters, cursor, end)
            cursor = end
            continue

        if source.startswith("b'", cursor):
            end = char_literal_end(source, cursor + 1)
            if end is not None:
                blank_range(characters, cursor, end)
                cursor = end
                continue

        if source[cursor] == "'":
            end = char_literal_end(source, cursor)
            if end is not None:
                blank_range(characters, cursor, end)
                cursor = end
                continue

        cursor += 1

    return "".join(characters)


def is_identifier_start(character: str) -> bool:
    return character == "_" or character.isalpha()


def is_identifier_continue(character: str) -> bool:
    return character == "_" or character.isalnum()


def tokenize(source: str) -> list[Token]:
    tokens: list[Token] = []
    cursor = 0
    line = 1
    column = 1
    while cursor < len(source):
        character = source[cursor]
        if character == "\n":
            cursor += 1
            line += 1
            column = 1
            continue
        if character.isspace():
            cursor += 1
            column += 1
            continue
        if (
            source.startswith("r#", cursor)
            and cursor + 2 < len(source)
            and is_identifier_start(source[cursor + 2])
        ):
            end = cursor + 3
            while end < len(source) and is_identifier_continue(source[end]):
                end += 1
            tokens.append(Token(source[cursor + 2 : end], line, column))
            column += end - cursor
            cursor = end
            continue
        if is_identifier_start(character):
            end = cursor + 1
            while end < len(source) and is_identifier_continue(source[end]):
                end += 1
            tokens.append(Token(source[cursor:end], line, column))
            column += end - cursor
            cursor = end
            continue
        if source.startswith("::", cursor):
            tokens.append(Token("::", line, column))
            cursor += 2
            column += 2
            continue
        tokens.append(Token(character, line, column))
        cursor += 1
        column += 1
    return tokens


class UseParser:
    def __init__(self, tokens: list[Token]) -> None:
        self.tokens = tokens
        self.position = 0

    def peek(self) -> str | None:
        if self.position >= len(self.tokens):
            return None
        return self.tokens[self.position].value

    def consume(self, expected: str | None = None) -> Token:
        if self.position >= len(self.tokens):
            raise ScanError("incomplete use statement")
        token = self.tokens[self.position]
        if expected is not None and token.value != expected:
            raise ScanError(f"expected '{expected}' in use statement")
        self.position += 1
        return token

    def parse(self) -> list[ImportPath]:
        paths = self.parse_tree([])
        if self.position != len(self.tokens):
            raise ScanError("unsupported tokens in use statement")
        return paths

    def parse_tree(self, prefix: list[Token]) -> list[ImportPath]:
        if self.peek() == "::":
            self.consume("::")
        if self.peek() == "{":
            return self.parse_group(prefix)

        segment = self.consume()
        if not (is_identifier_start(segment.value[0]) or segment.value == "*"):
            raise ScanError("expected path segment in use statement")
        path = prefix if segment.value == "self" and prefix else [*prefix, segment]

        if self.peek() == "::":
            self.consume("::")
            return (
                self.parse_group(path)
                if self.peek() == "{"
                else self.parse_tree(path)
            )

        alias = None
        if self.peek() == "as":
            self.consume("as")
            alias = self.consume()
            if not is_identifier_start(alias.value[0]):
                raise ScanError("invalid alias in use statement")
        return [ImportPath(path, alias)]

    def parse_group(self, prefix: list[Token]) -> list[ImportPath]:
        self.consume("{")
        paths: list[ImportPath] = []
        while self.peek() != "}":
            if self.peek() is None:
                raise ScanError("unterminated use group")
            paths.extend(self.parse_tree(prefix))
            if self.peek() == ",":
                self.consume(",")
            elif self.peek() != "}":
                raise ScanError("expected ',' or '}' in use group")
        self.consume("}")
        return paths


def violation_for_boundary_name(path: list[Token]) -> Violation | None:
    for token in path:
        if token.value in COMMAND_MODULES:
            return Violation("command or TUI modules", token)
    for token in path:
        if token.value in PRESENTATION_FRAMEWORKS:
            return Violation("presentation frameworks", token)
    for token in path:
        if token.value == "progress":
            return Violation("terminal progress", token)
    return None


def violation_for_path(path: list[Token]) -> Violation | None:
    violation = violation_for_boundary_name(path)
    if violation is not None:
        return violation
    values = [token.value for token in path]
    for index in range(len(values) - 2):
        if values[index : index + 2] == ["std", "io"] and values[index + 2] in TERMINAL_IO:
            return Violation("terminal I/O", path[index + 2])
    return None


def violation_for_import_path(path: list[Token]) -> Violation | None:
    violation = violation_for_path(path)
    if violation is not None:
        return violation
    values = [token.value for token in path]
    for index in range(len(values) - 1):
        imports_io_glob = (
            len(values) == index + 3 and values[index + 2] == "*"
        )
        if values[index : index + 2] == ["std", "io"] and imports_io_glob:
            return Violation("terminal I/O", path[index + 1])
    for token in path:
        if token.value in OUTPUT_MACROS:
            return Violation("terminal output macros", token)
    return None


def expand_path(path: list[Token], scopes: list[Scope]) -> list[Token] | None:
    expanded = path
    visited: set[tuple[int, str]] = set()
    while expanded:
        name = expanded[0].value
        binding: list[Token] | None = None
        binding_scope = -1
        for scope_index in range(len(scopes) - 1, -1, -1):
            scope = scopes[scope_index]
            if name in scope.aliases:
                binding = scope.aliases[name]
                binding_scope = scope_index
                break
            if name in scope.symbols:
                return None
        if binding is None:
            return expanded
        key = (binding_scope, name)
        if key in visited:
            return expanded
        visited.add(key)
        expanded = [*binding, *expanded[1:]]
    return expanded


def function_scope_symbols(
    tokens: list[Token], function_index: int
) -> tuple[int, set[str]] | None:
    name_index = function_index + 1
    if (
        name_index >= len(tokens)
        or not is_identifier_start(tokens[name_index].value[0])
    ):
        return None

    symbols: set[str] = set()
    cursor = name_index + 1
    if cursor < len(tokens) and tokens[cursor].value == "<":
        depth = 1
        cursor += 1
        parameter_start = True
        while cursor < len(tokens) and depth:
            value = tokens[cursor].value
            if value == "<":
                depth += 1
            elif value == ">":
                depth -= 1
            elif depth == 1 and value == ",":
                parameter_start = True
            elif depth == 1 and parameter_start:
                if value == "'":
                    parameter_start = False
                elif value == "const":
                    parameter_start = False
                elif is_identifier_start(value[0]):
                    symbols.add(value)
                    parameter_start = False
            cursor += 1
        if depth:
            raise ScanError("unterminated function generic parameters")

    while cursor < len(tokens):
        if tokens[cursor].value == ";":
            return None
        if tokens[cursor].value == "{":
            return cursor, symbols
        cursor += 1
    return None


def parse_use_statement(
    tokens: list[Token], use_index: int
) -> tuple[list[ImportPath], int]:
    end = use_index + 1
    while end < len(tokens) and tokens[end].value != ";":
        end += 1
    if end >= len(tokens):
        raise ScanError("unterminated use statement")
    return UseParser(tokens[use_index + 1 : end]).parse(), end


def parse_extern_crate(
    tokens: list[Token], extern_index: int
) -> tuple[Token, Token, int] | None:
    if (
        extern_index + 1 >= len(tokens)
        or tokens[extern_index + 1].value != "crate"
    ):
        return None
    if (
        extern_index + 2 >= len(tokens)
        or not is_identifier_start(tokens[extern_index + 2].value[0])
    ):
        raise ScanError("incomplete extern crate statement")
    crate_name = tokens[extern_index + 2]
    alias = crate_name
    end = extern_index + 3
    if end < len(tokens) and tokens[end].value == "as":
        end += 1
        if end >= len(tokens) or not is_identifier_start(tokens[end].value[0]):
            raise ScanError("invalid extern crate alias")
        alias = tokens[end]
        end += 1
    if end >= len(tokens) or tokens[end].value != ";":
        raise ScanError("unterminated extern crate statement")
    return crate_name, alias, end


def collect_item_scopes(tokens: list[Token]) -> tuple[Scope, dict[int, Scope]]:
    root = Scope.empty()
    scopes = [root]
    scopes_by_brace: dict[int, Scope] = {}
    pending_scope_symbols: dict[int, set[str]] = {}
    type_declarations = {"enum", "mod", "struct", "trait", "type", "union"}

    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.value == "{":
            scope = Scope.empty()
            for symbol in pending_scope_symbols.pop(index, set()):
                scope.bind_symbol(symbol)
            scopes_by_brace[index] = scope
            scopes.append(scope)
            index += 1
            continue
        if token.value == "}":
            if len(scopes) == 1:
                raise ScanError("unmatched closing brace")
            scopes.pop()
            index += 1
            continue

        if token.value == "use":
            imports, end = parse_use_statement(tokens, index)
            for imported in imports:
                local_name = imported.alias or imported.path[-1]
                if local_name.value != "*":
                    scopes[-1].bind_alias(local_name.value, imported.path)
            index = end + 1
            continue

        if token.value == "extern":
            external = parse_extern_crate(tokens, index)
            if external is not None:
                crate_name, alias, end = external
                scopes[-1].bind_alias(alias.value, [crate_name])
                index = end + 1
                continue

        if token.value in type_declarations and index + 1 < len(tokens):
            declaration = tokens[index + 1]
            if is_identifier_start(declaration.value[0]):
                scopes[-1].bind_symbol(declaration.value)

        if token.value == "fn":
            function_scope = function_scope_symbols(tokens, index)
            if function_scope is not None:
                body, symbols = function_scope
                pending_scope_symbols.setdefault(body, set()).update(symbols)

        index += 1

    if len(scopes) != 1:
        raise ScanError("unclosed brace")
    return root, scopes_by_brace


def scan_tokens(tokens: list[Token]) -> Violation | None:
    for index, token in enumerate(tokens):
        if token.value in OUTPUT_MACROS and index + 1 < len(tokens):
            if tokens[index + 1].value == "!":
                return Violation("terminal output macros", token)

    root_scope, scopes_by_brace = collect_item_scopes(tokens)
    scopes = [root_scope]
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.value == "{":
            scopes.append(scopes_by_brace[index])
            index += 1
            continue
        if token.value == "}":
            if len(scopes) == 1:
                raise ScanError("unmatched closing brace")
            scopes.pop()
            index += 1
            continue

        if token.value == "use":
            imports, end = parse_use_statement(tokens, index)
            for imported in imports:
                expanded = expand_path(imported.path, scopes)
                if expanded is not None:
                    violation = violation_for_import_path(expanded)
                    if violation is not None:
                        return violation
                else:
                    violation = violation_for_boundary_name(imported.path)
                    if violation is not None:
                        return violation
            index = end + 1
            continue

        if token.value == "extern":
            external = parse_extern_crate(tokens, index)
            if external is not None:
                crate_name, _alias, end = external
                violation = violation_for_path([crate_name])
                if violation is not None:
                    return violation
                index = end + 1
                continue

        if not is_identifier_start(token.value[0]):
            index += 1
            continue
        if (
            index >= 2
            and tokens[index - 1].value == "::"
            and is_identifier_start(tokens[index - 2].value[0])
        ):
            index += 1
            continue

        path = [token]
        end = index
        while (
            end + 2 < len(tokens)
            and tokens[end + 1].value == "::"
            and is_identifier_start(tokens[end + 2].value[0])
        ):
            path.append(tokens[end + 2])
            end += 2
        if len(path) > 1:
            expanded = expand_path(path, scopes)
            if expanded is not None:
                violation = violation_for_path(expanded)
                if violation is not None:
                    return violation
            else:
                violation = violation_for_boundary_name(path)
                if violation is not None:
                    return violation
        index = max(index + 1, end + 1)
    if len(scopes) != 1:
        raise ScanError("unclosed brace")
    return None


def application_sources(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        diagnostic = result.stderr.decode("utf-8", errors="replace").strip()
        raise ScanError(f"git source discovery failed: {diagnostic}")

    sources: list[Path] = []
    for raw_path in result.stdout.split(b"\0"):
        if not raw_path:
            continue
        try:
            relative = raw_path.decode("utf-8")
        except UnicodeDecodeError as error:
            raise ScanError(f"non-UTF-8 repository path: {error}") from error
        if relative.startswith("src/application/") and relative.endswith(".rs"):
            source = root / relative
            if source.exists():
                sources.append(source)
    return sorted(set(sources))


def scan_file(root: Path, path: Path) -> Violation | None:
    try:
        source = path.read_bytes().decode("utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise ScanError(f"could not read {path.relative_to(root)}: {error}") from error
    try:
        code = code_without_literals(source)
        return scan_tokens(tokenize(code))
    except ScanError as error:
        raise ScanError(f"could not scan {path.relative_to(root)}: {error}") from error


def main(arguments: list[str]) -> int:
    if len(arguments) != 1:
        print("usage: application-boundary-lint.py <repository-root>", file=sys.stderr)
        return 2

    try:
        root = Path(arguments[0]).expanduser().resolve(strict=True)
        if not root.is_dir():
            raise ScanError(f"repository root is not a directory: {root}")
        for path in application_sources(root):
            violation = scan_file(root, path)
            if violation is not None:
                relative = path.relative_to(root)
                token = violation.token
                print(
                    f"{relative}:{token.line}:{token.column}: forbidden application dependency",
                    file=sys.stderr,
                )
                print(
                    f"application boundary violation: {violation.label}",
                    file=sys.stderr,
                )
                return 1
    except (OSError, ScanError) as error:
        print(f"application boundary scanner error: {error}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
