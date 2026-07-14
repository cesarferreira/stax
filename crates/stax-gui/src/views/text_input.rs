use gpui::{
    App, Bounds, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, PaintQuad, ParentElement as _, Pixels, Render,
    ShapedLine, SharedString, Style, Styled as _, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, point, px, relative, rgba, size,
};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation as _;

actions!(
    branch_name_input,
    [Backspace, Delete, Left, Right, Home, End]
);

pub struct BranchNameInput {
    kind: TextInputKind,
    focus_handle: FocusHandle,
    text: SharedString,
    selected_range: Range<usize>,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextInputKind {
    BranchName,
    StackSearch,
}

impl TextInputKind {
    fn key_context(self) -> &'static str {
        match self {
            Self::BranchName => "BranchNameInput",
            Self::StackSearch => "StackSearchInput",
        }
    }
}

impl BranchNameInput {
    pub fn new(text: impl Into<SharedString>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = text.into();
        let focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        focus_handle.focus(window);
        let cursor = text.len();
        Self {
            kind: TextInputKind::BranchName,
            focus_handle,
            text,
            selected_range: cursor..cursor,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
        }
    }

    pub fn new_search(cx: &mut Context<Self>) -> Self {
        Self {
            kind: TextInputKind::StackSearch,
            focus_handle: cx.focus_handle().tab_index(10).tab_stop(true),
            text: SharedString::default(),
            selected_range: 0..0,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.text = text.into();
        let cursor = self.text.len();
        self.selected_range = cursor..cursor;
        self.marked_range = None;
        cx.notify();
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.selected_range = self.previous_boundary(self.cursor())..self.cursor();
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.selected_range = self.cursor()..self.next_boundary(self.cursor());
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected_range.start
        };
        self.move_to(cursor, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected_range.end
        };
        self.move_to(cursor, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.text.len(), cx);
    }

    fn cursor(&self) -> usize {
        self.selected_range.end
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = offset.min(self.text.len());
        self.selected_range = offset..offset;
        self.marked_range = None;
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.text.len())
    }

    fn replace_utf8_range(&mut self, range: Range<usize>, new_text: &str) -> Range<usize> {
        let mut text =
            String::with_capacity(self.text.len() - (range.end - range.start) + new_text.len());
        text.push_str(&self.text[..range.start]);
        text.push_str(new_text);
        text.push_str(&self.text[range.end..]);
        self.text = text.into();
        range.start..range.start + new_text.len()
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        offset_to_utf16_in(&self.text, offset)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        offset_from_utf16_in(&self.text, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = self.offset_from_utf16(range.start);
        let end = self.offset_from_utf16(range.end);
        start.min(end)..end.max(start)
    }
}

impl EntityInputHandler for BranchNameInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let inserted = self.replace_utf8_range(range, new_text);
        self.selected_range = inserted.end..inserted.end;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let inserted = self.replace_utf8_range(range, new_text);
        self.marked_range = (!new_text.is_empty()).then_some(inserted.clone());
        self.selected_range = new_selected_range.map_or(inserted.end..inserted.end, |range| {
            let start = offset_from_utf16_in(new_text, range.start);
            let end = offset_from_utf16_in(new_text, range.end);
            inserted.start + start..inserted.start + end
        });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line = self.last_layout.as_ref()?;
        let offset = line.index_for_x(point.x - bounds.left())?;
        Some(self.offset_to_utf16(offset))
    }
}

impl Focusable for BranchNameInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for BranchNameInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(self.kind.key_context())
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .w_full()
            .h(px(30.0))
            .px_2()
            .py_1()
            .child(BranchNameInputElement { input: cx.entity() })
    }
}

struct BranchNameInputElement {
    input: Entity<BranchNameInput>,
}

struct PrepaintState {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    marked: Option<PaintQuad>,
}

impl IntoElement for BranchNameInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BranchNameInputElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let run = TextRun {
            len: input.text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: input.text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(input.text.clone(), font_size, &runs, None);
        let cursor = input.selected_range.is_empty().then(|| {
            let x = line.x_for_index(input.selected_range.end);
            fill(
                Bounds::new(
                    point(bounds.left() + x, bounds.top()),
                    size(px(1.0), bounds.bottom() - bounds.top()),
                ),
                style.color,
            )
        });
        let marked = input.marked_range.as_ref().map(|range| {
            fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(range.start),
                        bounds.bottom() - px(2.0),
                    ),
                    point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
                ),
                rgba(0x1f72cf80),
            )
        });
        PrepaintState {
            line,
            cursor,
            marked,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(marked) = prepaint.marked.take() {
            window.paint_quad(marked);
        }
        prepaint
            .line
            .paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        let line = prepaint.line.clone();
        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

fn offset_to_utf16_in(text: &str, offset: usize) -> usize {
    text.char_indices()
        .take_while(|(index, _)| *index < offset)
        .map(|(_, character)| character.len_utf16())
        .sum()
}

fn offset_from_utf16_in(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, character) in text.char_indices() {
        if utf16 >= offset {
            return index;
        }
        let next = utf16 + character.len_utf16();
        if offset < next {
            return index;
        }
        utf16 = next;
    }
    text.len()
}
