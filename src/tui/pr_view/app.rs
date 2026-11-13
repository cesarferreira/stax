use crate::forge::MergeMethod;
use crate::github::pr_view::{Action, Detail, Entry, Group, PrRef, Source, dedupe, matches_filter};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    List,
    View,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Summary,
    Timeline,
    Code,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Summary, Tab::Timeline, Tab::Code];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn step(self, delta: isize) -> Tab {
        let len = Self::ALL.len() as isize;
        Self::ALL[(self.index() as isize + delta).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    None,
    Help,
    /// Pick how to merge.
    MergeMethod(Vec<MergeMethod>),
    /// Waiting for `y`/`n` before doing the action.
    Confirm(Action),
    /// Typing a comment.
    Comment(String),
}

#[derive(Debug, Clone)]
pub enum GroupStatus {
    Loading,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct GroupState {
    pub source: Source,
    pub status: GroupStatus,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    File,
    Meta,
    Hunk,
    Add,
    Del,
    Context,
}

#[derive(Debug, Clone)]
pub struct DiffRow {
    pub kind: DiffKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedPatch {
    pub rows: Vec<DiffRow>,
    /// Row index of each file header.
    pub files: Vec<usize>,
}

impl ParsedPatch {
    pub fn parse(patch: &str) -> Self {
        let mut out = ParsedPatch::default();
        let (mut old, mut new) = (0u32, 0u32);
        let mut in_hunk = false;
        for line in patch.lines() {
            let row = if let Some(rest) = line.strip_prefix("diff --git ") {
                in_hunk = false;
                out.files.push(out.rows.len());
                // "a/path b/path": show the new path.
                let name = rest.rsplit_once(" b/").map(|(_, b)| b).unwrap_or(rest);
                DiffRow {
                    kind: DiffKind::File,
                    old: None,
                    new: None,
                    text: name.to_owned(),
                }
            } else if let Some(rest) = line.strip_prefix("@@") {
                in_hunk = true;
                (old, new) = parse_hunk_start(rest);
                DiffRow {
                    kind: DiffKind::Hunk,
                    old: None,
                    new: None,
                    text: line.to_owned(),
                }
            } else if !in_hunk {
                DiffRow {
                    kind: DiffKind::Meta,
                    old: None,
                    new: None,
                    text: line.to_owned(),
                }
            } else if let Some(rest) = line.strip_prefix('+') {
                let row = DiffRow {
                    kind: DiffKind::Add,
                    old: None,
                    new: Some(new),
                    text: rest.to_owned(),
                };
                new += 1;
                row
            } else if let Some(rest) = line.strip_prefix('-') {
                let row = DiffRow {
                    kind: DiffKind::Del,
                    old: Some(old),
                    new: None,
                    text: rest.to_owned(),
                };
                old += 1;
                row
            } else if line.starts_with('\\') {
                DiffRow {
                    kind: DiffKind::Meta,
                    old: None,
                    new: None,
                    text: line.to_owned(),
                }
            } else {
                let row = DiffRow {
                    kind: DiffKind::Context,
                    old: Some(old),
                    new: Some(new),
                    text: line.strip_prefix(' ').unwrap_or(line).to_owned(),
                };
                old += 1;
                new += 1;
                row
            };
            out.rows.push(row);
        }
        out
    }
}

/// `-12,5 +14,7 @@ ...` (after the leading `@@`) to the first old and new line.
fn parse_hunk_start(rest: &str) -> (u32, u32) {
    let mut numbers = rest.split_whitespace().take(2).map(|part| {
        part.trim_start_matches(['-', '+'])
            .split(',')
            .next()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(1)
    });
    (numbers.next().unwrap_or(1), numbers.next().unwrap_or(1))
}

#[derive(Debug, Clone)]
pub enum PatchState {
    None,
    Loading,
    Ready(ParsedPatch),
    Failed(String),
}

pub struct App {
    pub screen: Screen,
    pub tab: Tab,
    pub overlay: Overlay,
    pub groups: Vec<GroupState>,
    pub filter: String,
    /// Index into the visible entries, headings not counted.
    pub cursor: usize,
    pub list_height: usize,
    pub current: Option<PrRef>,
    pub detail: Option<Detail>,
    pub detail_error: Option<String>,
    pub patch: PatchState,
    /// The head commit the patch was fetched for.
    pub patch_head: String,
    pub scroll: [usize; 3],
    pub hscroll: usize,
    /// Set by the renderer so keys can clamp and page.
    pub view_height: usize,
    pub content_len: usize,
    pub expanded: bool,
    pub status: Option<String>,
    pub busy: Option<String>,
    /// Opened on a named pull request: `q` quits instead of going back.
    pub direct: bool,
    pub list_dirty: bool,
    pub should_quit: bool,
    pub cache: HashMap<PrRef, Detail>,
    pub inflight: HashSet<PrRef>,
    /// Pull requests already read ahead from the list, so a failed read isn't retried every frame.
    pub prefetched: HashSet<PrRef>,
    /// The newest detail request per pull request; older answers are dropped.
    pub latest_detail: HashMap<PrRef, u64>,
    pub next_seq: u64,
    pub web_base: String,
}

impl App {
    pub fn new(sources: Vec<Source>, direct: Option<PrRef>, web_base: String) -> Self {
        let groups = sources
            .into_iter()
            .map(|source| GroupState {
                source,
                status: GroupStatus::Loading,
                entries: Vec::new(),
            })
            .collect();
        let mut app = App {
            screen: if direct.is_some() {
                Screen::View
            } else {
                Screen::List
            },
            tab: Tab::Summary,
            overlay: Overlay::None,
            groups,
            filter: String::new(),
            cursor: 0,
            list_height: 10,
            current: None,
            detail: None,
            detail_error: None,
            patch: PatchState::None,
            patch_head: String::new(),
            scroll: [0; 3],
            hscroll: 0,
            view_height: 10,
            content_len: 0,
            expanded: false,
            status: None,
            busy: None,
            direct: direct.is_some(),
            list_dirty: false,
            should_quit: false,
            cache: HashMap::new(),
            inflight: HashSet::new(),
            prefetched: HashSet::new(),
            latest_detail: HashMap::new(),
            next_seq: 0,
            web_base,
        };
        app.current = direct;
        app
    }

    /// The headings with their entries after the filter, in list order and
    /// without repeats. A heading with nothing to show still appears while it
    /// loads or when it failed.
    pub fn visible_groups(&self) -> Vec<Group> {
        let groups = self
            .groups
            .iter()
            .map(|g| Group {
                title: g.source.title(),
                entries: g.entries.clone(),
            })
            .collect();
        dedupe(groups)
            .into_iter()
            .map(|mut group| {
                group
                    .entries
                    .retain(|entry| matches_filter(entry, &self.filter));
                group
            })
            .collect()
    }

    pub fn visible_entries(&self) -> Vec<Entry> {
        self.visible_groups()
            .into_iter()
            .flat_map(|g| g.entries)
            .collect()
    }

    pub fn selected_entry(&self) -> Option<Entry> {
        self.visible_entries().into_iter().nth(self.cursor)
    }

    pub fn clamp_cursor(&mut self) {
        let len = self.visible_entries().len();
        self.cursor = self.cursor.min(len.saturating_sub(1));
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let len = self.visible_entries().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    pub fn url(&self, pr: &PrRef) -> String {
        let base = if pr.on_trusted {
            self.web_base.as_str()
        } else {
            "https://github.com"
        };
        format!("{base}/{}/pull/{}", pr.repo, pr.number)
    }

    pub fn scroll_mut(&mut self) -> &mut usize {
        &mut self.scroll[self.tab.index()]
    }

    pub fn max_scroll(&self) -> usize {
        self.content_len.saturating_sub(self.view_height.max(1))
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.max_scroll();
        let scroll = self.scroll_mut();
        *scroll = (*scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn apply_group(&mut self, index: usize, result: Result<Vec<Entry>, String>) {
        let Some(group) = self.groups.get_mut(index) else {
            return;
        };
        match result {
            Ok(entries) => {
                group.entries = entries;
                group.status = GroupStatus::Done;
            }
            Err(error) => group.status = GroupStatus::Failed(error),
        }
        self.clamp_cursor();
    }

    /// Shows `detail` if it is the pull request on screen.
    pub fn apply_detail(&mut self, pr: &PrRef, result: Result<Detail, String>) {
        self.inflight.remove(pr);
        match result {
            Ok(detail) => {
                if self.current.as_ref() == Some(pr) {
                    if detail.head_sha != self.patch_head {
                        self.patch = PatchState::None;
                    }
                    self.detail = Some(detail.clone());
                    self.detail_error = None;
                }
                self.cache.insert(pr.clone(), detail);
            }
            Err(error) => {
                if self.current.as_ref() == Some(pr) && self.detail.is_none() {
                    self.detail_error = Some(error);
                }
            }
        }
    }

    /// Enters the view for `pr`, showing what was read before while it is read again.
    pub fn open(&mut self, pr: PrRef) {
        self.detail = self.cache.get(&pr).cloned();
        self.detail_error = None;
        self.patch = PatchState::None;
        self.patch_head.clear();
        self.scroll = [0; 3];
        self.hscroll = 0;
        self.tab = Tab::Summary;
        self.overlay = Overlay::None;
        self.status = None;
        self.expanded = false;
        self.current = Some(pr);
        self.screen = Screen::View;
    }

    pub fn back_to_list(&mut self) {
        self.screen = Screen::List;
        self.overlay = Overlay::None;
        self.status = None;
        self.current = None;
        self.detail = None;
        self.detail_error = None;
        self.patch = PatchState::None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_rows_carry_line_numbers_and_file_offsets() {
        let patch = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,3 +10,4 @@ fn x()\n keep\n-gone\n+new\n+newer\n tail\ndiff --git a/b.txt b/b.txt\nnew file mode 100644\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+hello\n\\ No newline at end of file\n";
        let parsed = ParsedPatch::parse(patch);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.rows[parsed.files[0]].text, "src/a.rs");
        assert_eq!(parsed.rows[parsed.files[1]].text, "b.txt");

        let find = |kind, text: &str| {
            parsed
                .rows
                .iter()
                .find(|r| r.kind == kind && r.text == text)
                .unwrap()
        };
        let keep = find(DiffKind::Context, "keep");
        assert_eq!((keep.old, keep.new), (Some(10), Some(10)));
        let gone = find(DiffKind::Del, "gone");
        assert_eq!((gone.old, gone.new), (Some(11), None));
        let newer = find(DiffKind::Add, "newer");
        assert_eq!((newer.old, newer.new), (None, Some(12)));
        let tail = find(DiffKind::Context, "tail");
        assert_eq!((tail.old, tail.new), (Some(12), Some(13)));
        let hello = find(DiffKind::Add, "hello");
        assert_eq!(hello.new, Some(1));
        // `--- a/...` is file metadata, not a removed line.
        assert!(
            parsed
                .rows
                .iter()
                .any(|r| r.kind == DiffKind::Meta && r.text.starts_with("--- a/src"))
        );
        assert_eq!(
            parsed.rows.last().map(|r| r.kind),
            Some(DiffKind::Meta),
            "no-newline marker is not a context line"
        );
    }

    #[test]
    fn tabs_wrap_around() {
        assert_eq!(Tab::Summary.step(-1), Tab::Code);
        assert_eq!(Tab::Code.step(1), Tab::Summary);
        assert_eq!(Tab::Summary.step(1), Tab::Timeline);
    }

    fn entry(n: u64, title: &str) -> Entry {
        Entry {
            pr: PrRef {
                on_trusted: false,
                repo: "a/b".into(),
                number: n,
            },
            title: title.into(),
            author: "ada".into(),
            updated: format!("2026-01-0{n}T00:00:00Z"),
            draft: false,
        }
    }

    #[test]
    fn filter_and_cursor_follow_the_visible_entries() {
        let mut app = App::new(
            vec![Source::Repo("a/b".into()), Source::Yours],
            None,
            "https://github.com".into(),
        );
        app.apply_group(0, Ok(vec![entry(1, "alpha"), entry(2, "beta")]));
        app.apply_group(1, Ok(vec![entry(2, "beta"), entry(3, "gamma")]));
        // #2 shows once, under the first heading, newest first.
        let numbers: Vec<_> = app.visible_entries().iter().map(|e| e.pr.number).collect();
        assert_eq!(numbers, [2, 1, 3]);

        app.move_cursor(10);
        assert_eq!(app.selected_entry().unwrap().pr.number, 3);
        app.filter = "alp".into();
        app.clamp_cursor();
        assert_eq!(app.selected_entry().unwrap().pr.number, 1);
        app.filter = "zzz".into();
        app.clamp_cursor();
        assert!(app.selected_entry().is_none());
    }
}
