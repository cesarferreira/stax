#!/usr/bin/env python3
"""Enforce presentation-neutral Rust dependencies under src/application."""

from __future__ import annotations

import re
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
SOURCE_INJECTION_MACROS = {"include", "include_str", "include_bytes"}


class ScanError(Exception):
    """Raised when source cannot be scanned safely."""


@dataclass(frozen=True)
class Token:
    value: str
    line: int
    column: int
    offset: int


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
    module_id: int

    @classmethod
    def empty(cls, module_id: int) -> Scope:
        return cls({}, set(), module_id)

    def bind_alias(self, name: str, path: list[Token]) -> None:
        self.symbols.discard(name)
        self.aliases[name] = path

    def bind_symbol(self, name: str) -> None:
        self.aliases.pop(name, None)
        self.symbols.add(name)


@dataclass(frozen=True)
class ModuleDeclaration:
    name: Token
    parent_module_id: int
    child_module_id: int | None
    path_override: str | None


@dataclass
class ParsedSource:
    path: Path
    tokens: list[Token]
    root_scope: Scope
    scopes_by_brace: dict[int, Scope]
    module_scopes: list[Scope]
    module_parents: list[int | None]
    generic_headers: list[tuple[int, int, set[str]]]
    declarations: list[ModuleDeclaration]


@dataclass
class RepositoryModules:
    scopes: list[Scope]
    parents: list[int | None]
    children: dict[tuple[int, str], int]
    scanned: set[int]


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
            tokens.append(Token(source[cursor + 2 : end], line, column, cursor))
            column += end - cursor
            cursor = end
            continue
        if is_identifier_start(character):
            end = cursor + 1
            while end < len(source) and is_identifier_continue(source[end]):
                end += 1
            tokens.append(Token(source[cursor:end], line, column, cursor))
            column += end - cursor
            cursor = end
            continue
        if source.startswith("::", cursor):
            tokens.append(Token("::", line, column, cursor))
            cursor += 2
            column += 2
            continue
        tokens.append(Token(character, line, column, cursor))
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
        if token.value == "*":
            return Violation("glob imports", token)
    for token in path:
        if token.value in OUTPUT_MACROS:
            return Violation("terminal output macros", token)
    for token in path:
        if token.value in SOURCE_INJECTION_MACROS:
            return Violation("source injection macros", token)
    return None


def expand_path(
    path: list[Token],
    scopes: list[Scope],
    module_scopes: list[Scope],
    module_parents: list[int | None],
    module_children: dict[tuple[int, str], int],
) -> list[Token] | None:
    expanded = path
    visited: set[tuple[int, str]] = set()
    lookup_scopes = scopes
    source_root_scope = scopes[0]
    module_id = scopes[-1].module_id
    while expanded:
        if expanded[0].value in {"crate", "self", "super"}:
            qualifier = expanded[0].value
            unknown_ancestor = False
            if qualifier == "crate":
                module_id = 0
                expanded = expanded[1:]
            elif qualifier == "self":
                expanded = expanded[1:]
            else:
                while expanded and expanded[0].value == "super":
                    parent = module_parents[module_id]
                    if parent is None:
                        unknown_ancestor = True
                        expanded = expanded[1:]
                        continue
                    module_id = parent
                    expanded = expanded[1:]
            if unknown_ancestor:
                lookup_scopes = []
            elif qualifier == "crate" and source_root_scope is not module_scopes[0]:
                lookup_scopes = [source_root_scope, module_scopes[0]]
            else:
                lookup_scopes = [module_scopes[module_id]]
            if not expanded:
                return expanded

        name = expanded[0].value
        binding: list[Token] | None = None
        binding_scope = -1
        child_module: int | None = None
        for scope_index in range(len(lookup_scopes) - 1, -1, -1):
            scope = lookup_scopes[scope_index]
            if name in scope.aliases:
                binding = scope.aliases[name]
                binding_scope = scope_index
                break
            child_module = module_children.get((scope.module_id, name))
            if (
                child_module is not None
                and scope is module_scopes[scope.module_id]
            ):
                break
            if name in scope.symbols:
                return None
        if child_module is not None:
            if violation_for_boundary_name([expanded[0]]) is not None:
                return expanded
            module_id = child_module
            lookup_scopes = [module_scopes[module_id]]
            expanded = expanded[1:]
            if not expanded:
                return expanded
            continue
        if binding is None:
            return expanded
        key = (id(lookup_scopes[binding_scope]), name)
        if key in visited:
            return expanded
        visited.add(key)
        module_id = lookup_scopes[binding_scope].module_id
        lookup_scopes = lookup_scopes[: binding_scope + 1]
        expanded = [*binding, *expanded[1:]]
    return expanded


def generic_declaration_scope(
    tokens: list[Token], declaration_index: int
) -> tuple[int, int, set[str], int | None] | None:
    declaration = tokens[declaration_index].value
    if declaration not in {"enum", "fn", "impl", "struct", "trait", "type", "union"}:
        return None

    generic_open = declaration_index + 1
    if declaration != "impl":
        if (
            generic_open >= len(tokens)
            or not is_identifier_start(tokens[generic_open].value[0])
        ):
            return None
        generic_open += 1
    if generic_open >= len(tokens) or tokens[generic_open].value != "<":
        return None

    symbols: set[str] = set()
    depth = 1
    cursor = generic_open + 1
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
            if value in {"'", "const"}:
                parameter_start = False
            elif is_identifier_start(value[0]):
                symbols.add(value)
                parameter_start = False
        cursor += 1
    if depth:
        raise ScanError("unterminated generic parameters")

    parentheses = 0
    brackets = 0
    angles = 0
    while cursor < len(tokens):
        value = tokens[cursor].value
        if value == "(":
            parentheses += 1
        elif value == ")":
            parentheses = max(0, parentheses - 1)
        elif value == "[":
            brackets += 1
        elif value == "]":
            brackets = max(0, brackets - 1)
        elif value == "<":
            angles += 1
        elif value == ">":
            angles = max(0, angles - 1)
        elif not (parentheses or brackets or angles):
            if value == "{":
                return generic_open + 1, cursor, symbols, cursor
            if value == ";":
                return generic_open + 1, cursor, symbols, None
        cursor += 1
    raise ScanError("unterminated generic declaration")


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


def collect_item_scopes(
    tokens: list[Token],
) -> tuple[
    Scope,
    dict[int, Scope],
    list[Scope],
    list[int | None],
    list[tuple[int, int, set[str]]],
]:
    root = Scope.empty(0)
    scopes = [root]
    scopes_by_brace: dict[int, Scope] = {}
    module_scopes = [root]
    module_parents: list[int | None] = [None]
    module_braces: set[int] = set()
    pending_scope_symbols: dict[int, set[str]] = {}
    generic_headers: list[tuple[int, int, set[str]]] = []
    type_declarations = {"enum", "mod", "struct", "trait", "type", "union"}

    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.value == "{":
            if index in module_braces:
                module_id = len(module_scopes)
                module_parents.append(scopes[-1].module_id)
                scope = Scope.empty(module_id)
                module_scopes.append(scope)
            else:
                scope = Scope.empty(scopes[-1].module_id)
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
                if (
                    token.value == "mod"
                    and index + 2 < len(tokens)
                    and tokens[index + 2].value == "{"
                ):
                    module_braces.add(index + 2)

        generic_scope = generic_declaration_scope(tokens, index)
        if generic_scope is not None:
            start, end, symbols, body = generic_scope
            generic_headers.append((start, end, symbols))
            if body is not None:
                pending_scope_symbols.setdefault(body, set()).update(symbols)

        index += 1

    if len(scopes) != 1:
        raise ScanError("unclosed brace")
    return root, scopes_by_brace, module_scopes, module_parents, generic_headers


def module_path_override(
    source: str, tokens: list[Token], module_index: int
) -> str | None:
    matches: list[tuple[int, int]] = []
    start = max(0, module_index - 32)
    for index in range(start, module_index - 4):
        values = [token.value for token in tokens[index : index + 5]]
        if values != ["#", "[", "path", "=", "]"]:
            continue
        if any(
            token.value in {";", "{", "}"}
            for token in tokens[index + 5 : module_index]
        ):
            continue
        matches.append((tokens[index].offset, tokens[index + 4].offset + 1))
    if not matches:
        return None
    if len(matches) > 1:
        raise ScanError("multiple path attributes on module")
    attribute = source[matches[0][0] : matches[0][1]]
    path_match = re.search(r'path\s*=\s*"([^"\\]*)"', attribute, re.DOTALL)
    if path_match is None:
        raise ScanError("unsupported path attribute literal")
    return path_match.group(1)


def collect_module_declarations(
    source: str,
    tokens: list[Token],
    root_scope: Scope,
    scopes_by_brace: dict[int, Scope],
) -> list[ModuleDeclaration]:
    declarations: list[ModuleDeclaration] = []
    scopes = [root_scope]
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token.value == "{":
            scopes.append(scopes_by_brace[index])
            index += 1
            continue
        if token.value == "}":
            scopes.pop()
            index += 1
            continue
        if token.value == "use":
            _imports, end = parse_use_statement(tokens, index)
            index = end + 1
            continue
        if token.value != "mod":
            index += 1
            continue
        if (
            index + 2 >= len(tokens)
            or not is_identifier_start(tokens[index + 1].value[0])
        ):
            index += 1
            continue
        terminator = tokens[index + 2].value
        if terminator not in {"{", ";"}:
            raise ScanError("malformed module declaration")
        override = module_path_override(source, tokens, index)
        if override is not None and terminator == "{":
            raise ScanError("path attribute on inline module")
        declarations.append(
            ModuleDeclaration(
                name=tokens[index + 1],
                parent_module_id=scopes[-1].module_id,
                child_module_id=(
                    scopes_by_brace[index + 2].module_id
                    if terminator == "{"
                    else None
                ),
                path_override=override,
            )
        )
        index += 1
    return declarations


def explicit_macro_import_path(
    path: list[Token], scopes: list[Scope]
) -> tuple[list[Token], int] | None:
    expanded = path
    lookup_scopes = scopes
    module_id = scopes[-1].module_id
    visited: set[tuple[int, str]] = set()
    while expanded:
        if expanded[0].value in {"crate", "self", "super"}:
            return expanded, module_id
        name = expanded[0].value
        binding: list[Token] | None = None
        binding_scope = -1
        for scope_index in range(len(lookup_scopes) - 1, -1, -1):
            scope = lookup_scopes[scope_index]
            if name in scope.aliases:
                binding = scope.aliases[name]
                binding_scope = scope_index
                break
            if name in scope.symbols:
                return None
        if binding is None:
            return None
        key = (id(lookup_scopes[binding_scope]), name)
        if key in visited:
            return None
        visited.add(key)
        module_id = lookup_scopes[binding_scope].module_id
        lookup_scopes = lookup_scopes[: binding_scope + 1]
        expanded = [*binding, *expanded[1:]]
    return None


def explicit_macro_source_is_unknown(
    path: list[Token],
    origin_module_id: int,
    module_parents: list[int | None],
    module_children: dict[tuple[int, str], int],
    scanned_modules: set[int],
) -> bool:
    module_id = origin_module_id
    if path[0].value == "crate":
        module_id = 0
        path = path[1:]
    elif path[0].value == "self":
        path = path[1:]
    else:
        while path and path[0].value == "super":
            parent = module_parents[module_id]
            if parent is None:
                return True
            module_id = parent
            path = path[1:]
    for segment in path[:-1]:
        child = module_children.get((module_id, segment.value))
        if child is None:
            return True
        module_id = child
    return module_id not in scanned_modules


def scan_tokens(
    tokens: list[Token],
    root_scope: Scope | None = None,
    scopes_by_brace: dict[int, Scope] | None = None,
    module_scopes: list[Scope] | None = None,
    module_parents: list[int | None] | None = None,
    generic_headers: list[tuple[int, int, set[str]]] | None = None,
    module_children: dict[tuple[int, str], int] | None = None,
    scanned_modules: set[int] | None = None,
) -> Violation | None:
    for index, token in enumerate(tokens):
        defines_macro_rules = (
            token.value == "macro_rules"
            and index + 1 < len(tokens)
            and tokens[index + 1].value == "!"
        )
        defines_macro_item = (
            token.value == "macro"
            and index + 1 < len(tokens)
            and is_identifier_start(tokens[index + 1].value[0])
        )
        if defines_macro_rules or defines_macro_item:
            return Violation("local declarative macros", token)

    for index, token in enumerate(tokens):
        if token.value in OUTPUT_MACROS and index + 1 < len(tokens):
            if tokens[index + 1].value == "!":
                return Violation("terminal output macros", token)
        if token.value in SOURCE_INJECTION_MACROS and index + 1 < len(tokens):
            if tokens[index + 1].value == "!":
                return Violation("source injection macros", token)

    if root_scope is None:
        (
            root_scope,
            scopes_by_brace,
            module_scopes,
            module_parents,
            generic_headers,
        ) = collect_item_scopes(tokens)
    if (
        scopes_by_brace is None
        or module_scopes is None
        or module_parents is None
        or generic_headers is None
    ):
        raise ScanError("incomplete scanner scope configuration")
    module_children = module_children or {}
    scanned_modules = scanned_modules or set(range(len(module_scopes)))
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
                expanded = expand_path(
                    imported.path,
                    scopes,
                    module_scopes,
                    module_parents,
                    module_children,
                )
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

        path_scopes = scopes
        generic_symbols = [
            symbols
            for start, end, symbols in generic_headers
            if start <= index < end
        ]
        if generic_symbols:
            path_scopes = [*scopes]
            for symbols in generic_symbols:
                generic_scope = Scope.empty(scopes[-1].module_id)
                for symbol in symbols:
                    generic_scope.bind_symbol(symbol)
                path_scopes.append(generic_scope)

        path = [token]
        end = index
        while (
            end + 2 < len(tokens)
            and tokens[end + 1].value == "::"
            and is_identifier_start(tokens[end + 2].value[0])
        ):
            path.append(tokens[end + 2])
            end += 2
        if end + 1 < len(tokens) and tokens[end + 1].value == "!":
            explicit_import = explicit_macro_import_path(path, path_scopes)
            if explicit_import is not None:
                imported_path, origin_module_id = explicit_import
                if explicit_macro_source_is_unknown(
                    imported_path,
                    origin_module_id,
                    module_parents,
                    module_children,
                    scanned_modules,
                ):
                    return Violation("unknown source macro imports", path[0])
        if len(path) > 1:
            expanded = expand_path(
                path,
                path_scopes,
                module_scopes,
                module_parents,
                module_children,
            )
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


def reject_dynamic_module_source_attributes(tokens: list[Token]) -> None:
    index = 0
    while index + 1 < len(tokens):
        if tokens[index].value != "#" or tokens[index + 1].value != "[":
            index += 1
            continue
        depth = 1
        end = index + 2
        while end < len(tokens) and depth:
            if tokens[end].value == "[":
                depth += 1
            elif tokens[end].value == "]":
                depth -= 1
            end += 1
        if depth:
            raise ScanError("unterminated attribute")
        values = [token.value for token in tokens[index + 2 : end - 1]]
        has_path_assignment = any(
            values[position : position + 2] == ["path", "="]
            for position in range(len(values) - 1)
        )
        if has_path_assignment and values and values[0] == "cfg_attr":
            raise ScanError("conditional module source attribute is unsupported")
        if has_path_assignment and values[:2] != ["path", "="]:
            raise ScanError("dynamic module source attribute is unsupported")
        if values[:2] == ["path", "="] and len(values) != 2:
            raise ScanError("dynamic module source attribute is unsupported")
        index = end


def parse_source(root: Path, path: Path) -> ParsedSource:
    try:
        source = path.read_bytes().decode("utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise ScanError(f"could not read {path.relative_to(root)}: {error}") from error
    try:
        code = code_without_literals(source)
        tokens = tokenize(code)
        reject_dynamic_module_source_attributes(tokens)
        (
            root_scope,
            scopes_by_brace,
            module_scopes,
            module_parents,
            generic_headers,
        ) = collect_item_scopes(tokens)
        declarations = collect_module_declarations(
            source, tokens, root_scope, scopes_by_brace
        )
        return ParsedSource(
            path=path.resolve(),
            tokens=tokens,
            root_scope=root_scope,
            scopes_by_brace=scopes_by_brace,
            module_scopes=module_scopes,
            module_parents=module_parents,
            generic_headers=generic_headers,
            declarations=declarations,
        )
    except ScanError as error:
        raise ScanError(f"could not scan {path.relative_to(root)}: {error}") from error


def inferred_module_path(application_root: Path, path: Path) -> tuple[str, ...]:
    relative = path.relative_to(application_root)
    if relative.name == "mod.rs":
        components = relative.parent.parts
    else:
        components = (*relative.parent.parts, relative.stem)
    return ("application", *components)


def build_repository_modules(
    root: Path, parsed_sources: list[ParsedSource]
) -> RepositoryModules:
    application_root = (root / "src" / "application").resolve()
    sources_by_path = {source.path: source for source in parsed_sources}
    module_owners: dict[tuple[str, ...], tuple[Path, int]] = {}
    file_roots: dict[Path, tuple[str, ...]] = {}
    local_paths_by_file: dict[Path, dict[int, tuple[str, ...]]] = {}
    declarations_seen: set[tuple[tuple[str, ...], str]] = set()
    processing: set[Path] = set()
    processed: set[Path] = set()

    def claim_module(
        module_path: tuple[str, ...], owner: tuple[Path, int]
    ) -> None:
        previous = module_owners.get(module_path)
        if previous is not None and previous != owner:
            rendered = "::".join(module_path)
            raise ScanError(f"duplicate module path: {rendered}")
        module_owners[module_path] = owner

    def module_source(
        source: ParsedSource,
        parent_suffix: tuple[str, ...],
        declaration: ModuleDeclaration,
    ) -> Path:
        if declaration.path_override is not None:
            module_directory = source.path.parent
            if parent_suffix:
                if source.path.name != "mod.rs":
                    module_directory /= source.path.stem
                module_directory = module_directory.joinpath(*parent_suffix)
            candidate = (
                module_directory / declaration.path_override
            ).resolve()
            try:
                candidate.relative_to(application_root)
            except ValueError as error:
                raise ScanError(
                    f"module path escapes src/application: {declaration.path_override}"
                ) from error
            if not candidate.exists() or not candidate.is_file():
                raise ScanError(
                    f"module source is unreadable or missing: {candidate}"
                )
            if candidate not in sources_by_path:
                raise ScanError(f"module source was not scanned: {candidate}")
            return candidate

        if source.path.name == "mod.rs":
            module_directory = source.path.parent
        else:
            module_directory = source.path.parent / source.path.stem
        module_directory = module_directory.joinpath(*parent_suffix)
        candidates = [
            (module_directory / f"{declaration.name.value}.rs").resolve(),
            (
                module_directory
                / declaration.name.value
                / "mod.rs"
            ).resolve(),
        ]
        existing = [candidate for candidate in candidates if candidate.exists()]
        if len(existing) > 1:
            raise ScanError(
                f"ambiguous module source for {declaration.name.value}"
            )
        if not existing or not existing[0].is_file():
            raise ScanError(
                f"module source is unreadable or missing: {declaration.name.value}"
            )
        if existing[0] not in sources_by_path:
            raise ScanError(f"module source was not scanned: {existing[0]}")
        return existing[0]

    def assign_file(path: Path, root_module: tuple[str, ...]) -> None:
        previous_root = file_roots.get(path)
        if previous_root is not None:
            if previous_root != root_module:
                raise ScanError(
                    f"module file has multiple paths: {path}"
                )
            return
        if path in processing:
            raise ScanError(f"cyclic module source mapping: {path}")
        processing.add(path)
        file_roots[path] = root_module
        source = sources_by_path[path]

        local_paths: dict[int, tuple[str, ...]] = {0: root_module}
        local_suffixes: dict[int, tuple[str, ...]] = {0: ()}
        inline = [
            declaration
            for declaration in source.declarations
            if declaration.child_module_id is not None
        ]
        while inline:
            remaining: list[ModuleDeclaration] = []
            progress = False
            for declaration in inline:
                parent = local_paths.get(declaration.parent_module_id)
                suffix = local_suffixes.get(declaration.parent_module_id)
                if parent is None or suffix is None:
                    remaining.append(declaration)
                    continue
                child = (*parent, declaration.name.value)
                child_id = declaration.child_module_id
                if child_id is None:
                    raise ScanError("inline module is missing its scope")
                previous = local_paths.get(child_id)
                if previous is not None and previous != child:
                    raise ScanError("ambiguous inline module path")
                local_paths[child_id] = child
                local_suffixes[child_id] = (*suffix, declaration.name.value)
                claim_module(child, (path, child_id))
                progress = True
            if not progress and remaining:
                raise ScanError("could not resolve inline module parent")
            inline = remaining

        claim_module(root_module, (path, 0))
        local_paths_by_file[path] = local_paths

        for declaration in source.declarations:
            parent = local_paths.get(declaration.parent_module_id)
            if parent is None:
                raise ScanError("could not resolve module declaration parent")
            edge = (parent, declaration.name.value)
            if edge in declarations_seen:
                rendered = "::".join((*parent, declaration.name.value))
                raise ScanError(f"duplicate module declaration: {rendered}")
            declarations_seen.add(edge)
            if declaration.child_module_id is not None:
                continue
            parent_suffix = local_suffixes[declaration.parent_module_id]
            child_source = module_source(source, parent_suffix, declaration)
            assign_file(child_source, (*parent, declaration.name.value))

        processing.remove(path)
        processed.add(path)

    application_mod = (application_root / "mod.rs").resolve()
    if application_mod in sources_by_path:
        assign_file(application_mod, ("application",))

    for path in sorted(sources_by_path):
        if path not in processed and path not in file_roots:
            assign_file(path, inferred_module_path(application_root, path))

    all_paths: set[tuple[str, ...]] = {(), ("application",)}
    for module_path in module_owners:
        for length in range(len(module_path) + 1):
            all_paths.add(module_path[:length])
    ordered_paths = sorted(all_paths, key=lambda value: (len(value), value))
    path_ids = {
        module_path: module_id
        for module_id, module_path in enumerate(ordered_paths)
    }

    global_scopes: list[Scope] = []
    for module_id, module_path in enumerate(ordered_paths):
        owner = module_owners.get(module_path)
        if owner is None:
            global_scopes.append(Scope.empty(module_id))
            continue
        owner_path, local_id = owner
        global_scopes.append(
            sources_by_path[owner_path].module_scopes[local_id]
        )

    for path, source in sources_by_path.items():
        local_paths = local_paths_by_file[path]
        local_ids = {
            local_id: path_ids[module_path]
            for local_id, module_path in local_paths.items()
        }
        scopes = {id(source.root_scope): source.root_scope}
        scopes.update(
            {id(scope): scope for scope in source.scopes_by_brace.values()}
        )
        for scope in scopes.values():
            scope.module_id = local_ids[scope.module_id]

    parents: list[int | None] = []
    children: dict[tuple[int, str], int] = {}
    for module_path in ordered_paths:
        if not module_path:
            parents.append(None)
            continue
        parent_id = path_ids[module_path[:-1]]
        module_id = path_ids[module_path]
        parents.append(parent_id)
        children[(parent_id, module_path[-1])] = module_id
    scanned = {
        path_ids[module_path]
        for module_path in module_owners
    }
    return RepositoryModules(global_scopes, parents, children, scanned)


def scan_parsed_source(
    source: ParsedSource, modules: RepositoryModules
) -> Violation | None:
    return scan_tokens(
        source.tokens,
        root_scope=source.root_scope,
        scopes_by_brace=source.scopes_by_brace,
        module_scopes=modules.scopes,
        module_parents=modules.parents,
        generic_headers=source.generic_headers,
        module_children=modules.children,
        scanned_modules=modules.scanned,
    )


def main(arguments: list[str]) -> int:
    if len(arguments) != 1:
        print("usage: application-boundary-lint.py <repository-root>", file=sys.stderr)
        return 2

    try:
        root = Path(arguments[0]).expanduser().resolve(strict=True)
        if not root.is_dir():
            raise ScanError(f"repository root is not a directory: {root}")
        parsed_sources = [
            parse_source(root, path) for path in application_sources(root)
        ]
        modules = build_repository_modules(root, parsed_sources)
        for source in parsed_sources:
            violation = scan_parsed_source(source, modules)
            if violation is not None:
                relative = source.path.relative_to(root)
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
