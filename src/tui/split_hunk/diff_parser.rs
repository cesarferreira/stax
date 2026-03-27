#[derive(Debug, Clone)]
pub struct DiffFile {
    pub path: String,
    pub header_lines: Vec<String>,
    pub is_new: bool,
    pub is_deleted: bool,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<String>,
    pub new_start: u32,
    pub new_count: u32,
}

pub fn parse_diff(diff_text: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let lines: Vec<&str> = diff_text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if !lines[i].starts_with("diff --git ") {
            i += 1;
            continue;
        }

        let mut header_lines = vec![lines[i].to_string()];
        let mut is_new = false;
        let mut is_deleted = false;
        let mut path = extract_path_from_diff_line(lines[i]);
        i += 1;

        while i < lines.len() && !lines[i].starts_with("diff --git ") && !lines[i].starts_with("@@")
        {
            let line = lines[i];
            header_lines.push(line.to_string());

            if line.starts_with("new file mode") {
                is_new = true;
            } else if line.starts_with("deleted file mode") {
                is_deleted = true;
            } else if let Some(stripped) = line.strip_prefix("+++ b/") {
                path = stripped.to_string();
            } else if is_deleted && line.starts_with("+++ /dev/null") {
            } else if is_deleted {
                if let Some(stripped) = line.strip_prefix("--- a/") {
                    path = stripped.to_string();
                }
            }

            i += 1;
        }

        let mut hunks = Vec::new();

        while i < lines.len() && !lines[i].starts_with("diff --git ") {
            if lines[i].starts_with("@@") {
                let header = lines[i].to_string();
                let (new_start, new_count) = parse_hunk_header(lines[i]);
                i += 1;

                let mut hunk_lines = Vec::new();
                while i < lines.len()
                    && !lines[i].starts_with("@@")
                    && !lines[i].starts_with("diff --git ")
                {
                    hunk_lines.push(lines[i].to_string());
                    i += 1;
                }

                hunks.push(DiffHunk {
                    header,
                    lines: hunk_lines,
                    new_start,
                    new_count,
                });
            } else {
                i += 1;
            }
        }

        if hunks.is_empty() && (is_new || is_deleted) {
            hunks.push(DiffHunk {
                header: String::new(),
                lines: Vec::new(),
                new_start: 0,
                new_count: 0,
            });
        }

        files.push(DiffFile {
            path,
            header_lines,
            is_new,
            is_deleted,
            hunks,
        });
    }

    files
}

fn extract_path_from_diff_line(line: &str) -> String {
    let rest = &line["diff --git ".len()..];
    if let Some(b_pos) = rest.find(" b/") {
        rest[b_pos + " b/".len()..].to_string()
    } else {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() == 2 {
            parts[1].strip_prefix("b/").unwrap_or(parts[1]).to_string()
        } else {
            rest.to_string()
        }
    }
}

fn parse_hunk_header(line: &str) -> (u32, u32) {
    let after_plus = match line.find('+') {
        Some(pos) => &line[pos + 1..],
        None => return (1, 0),
    };

    let nums_end = after_plus
        .find(|c: char| c != ',' && !c.is_ascii_digit())
        .unwrap_or(after_plus.len());
    let nums = &after_plus[..nums_end];

    let parts: Vec<&str> = nums.splitn(2, ',').collect();
    let start = parts[0].parse::<u32>().unwrap_or(1);
    let count = if parts.len() > 1 {
        parts[1].parse::<u32>().unwrap_or(0)
    } else {
        1
    };

    (start, count)
}

pub fn reconstruct_patch(file: &DiffFile, hunk_indices: &[usize]) -> String {
    let mut out = String::new();

    for header_line in &file.header_lines {
        out.push_str(header_line);
        out.push('\n');
    }

    for &idx in hunk_indices {
        if let Some(hunk) = file.hunks.get(idx) {
            if !hunk.header.is_empty() {
                out.push_str(&hunk.header);
                out.push('\n');
            }
            for line in &hunk.lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    out
}

pub fn reconstruct_full_patch(files: &[DiffFile], selections: &[(usize, Vec<usize>)]) -> String {
    let mut out = String::new();

    for (file_idx, hunk_indices) in selections {
        if let Some(file) = files.get(*file_idx) {
            out.push_str(&reconstruct_patch(file, hunk_indices));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_FILE_SINGLE_HUNK: &str = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }";

    const SINGLE_FILE_MULTIPLE_HUNKS: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 use std::io;
+use std::fs;

 fn foo() {
     bar();
@@ -20,6 +21,7 @@
 fn baz() {
     let a = 1;
+    let b = 2;
     let c = 3;
 }";

    const MULTIPLE_FILES: &str = "\
diff --git a/src/a.rs b/src/a.rs
index aaa..bbb 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 fn a() {
+    // a
 }
diff --git a/src/b.rs b/src/b.rs
index ccc..ddd 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -5,3 +5,4 @@
 fn b() {
+    // b
 }";

    const NEW_FILE: &str = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..abc1234
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,3 @@
+fn new_func() {
+    todo!()
+}";

    #[test]
    fn test_parse_single_file_single_hunk() {
        let files = parse_diff(SINGLE_FILE_SINGLE_HUNK);
        assert_eq!(files.len(), 1);

        let file = &files[0];
        assert_eq!(file.path, "src/main.rs");
        assert!(!file.is_new);
        assert!(!file.is_deleted);
        assert_eq!(file.hunks.len(), 1);

        let hunk = &file.hunks[0];
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.new_count, 4);
        assert_eq!(hunk.lines.len(), 4);
    }

    #[test]
    fn test_parse_multiple_hunks() {
        let files = parse_diff(SINGLE_FILE_MULTIPLE_HUNKS);
        assert_eq!(files.len(), 1);

        let file = &files[0];
        assert_eq!(file.path, "src/lib.rs");
        assert_eq!(file.hunks.len(), 2);

        assert_eq!(file.hunks[0].new_start, 1);
        assert_eq!(file.hunks[0].new_count, 5);
        assert_eq!(file.hunks[1].new_start, 21);
        assert_eq!(file.hunks[1].new_count, 7);
    }

    #[test]
    fn test_parse_multiple_files() {
        let files = parse_diff(MULTIPLE_FILES);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[1].path, "src/b.rs");
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[1].hunks.len(), 1);
        assert_eq!(files[1].hunks[0].new_start, 5);
    }

    #[test]
    fn test_parse_new_file() {
        let files = parse_diff(NEW_FILE);
        assert_eq!(files.len(), 1);

        let file = &files[0];
        assert_eq!(file.path, "src/new.rs");
        assert!(file.is_new);
        assert!(!file.is_deleted);
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(file.hunks[0].new_start, 1);
        assert_eq!(file.hunks[0].new_count, 3);
    }

    #[test]
    fn test_reconstruct_patch_single_hunk() {
        let files = parse_diff(SINGLE_FILE_MULTIPLE_HUNKS);
        let file = &files[0];

        let patch = reconstruct_patch(file, &[0]);
        assert!(patch.contains("@@ -1,4 +1,5 @@"));
        assert!(patch.contains("+use std::fs;"));
        assert!(!patch.contains("@@ -20,6 +21,7 @@"));
        assert!(!patch.contains("+    let b = 2;"));

        let patch_both = reconstruct_patch(file, &[0, 1]);
        assert!(patch_both.contains("@@ -1,4 +1,5 @@"));
        assert!(patch_both.contains("@@ -20,6 +21,7 @@"));
    }

    #[test]
    fn test_parse_empty_diff() {
        assert!(parse_diff("").is_empty());
        assert!(parse_diff("\n\n").is_empty());
    }

    #[test]
    fn test_parse_deleted_file() {
        let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index abc1234..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn old() {
-    // removed
-}";
        let files = parse_diff(diff);
        assert_eq!(files.len(), 1);
        assert!(files[0].is_deleted);
        assert!(!files[0].is_new);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].lines.len(), 3);
    }

    #[test]
    fn test_reconstruct_full_patch_multiple_files() {
        let files = parse_diff(MULTIPLE_FILES);
        let selections = vec![(0, vec![0]), (1, vec![0])];
        let patch = reconstruct_full_patch(&files, &selections);

        assert!(patch.contains("diff --git a/src/a.rs"));
        assert!(patch.contains("diff --git a/src/b.rs"));
        assert!(patch.contains("+    // a"));
        assert!(patch.contains("+    // b"));
    }

    #[test]
    fn test_reconstruct_full_patch_selective() {
        let files = parse_diff(MULTIPLE_FILES);
        let selections = vec![(0, vec![0])];
        let patch = reconstruct_full_patch(&files, &selections);

        assert!(patch.contains("diff --git a/src/a.rs"));
        assert!(!patch.contains("diff --git a/src/b.rs"));
    }

    #[test]
    fn test_reconstruct_full_patch_empty_selections() {
        let files = parse_diff(MULTIPLE_FILES);
        let selections: Vec<(usize, Vec<usize>)> = vec![];
        let patch = reconstruct_full_patch(&files, &selections);
        assert!(patch.is_empty());
    }
}
