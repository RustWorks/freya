use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

use dioxus_core::{AttributeValue, Scope, ScopeState};
use dioxus_hooks::{to_owned, use_effect, use_state, UseState};
use freya_common::{CursorLayoutResponse, EventMessage};
use freya_elements::events::{KeyboardData, MouseData};
use freya_node_state::{CursorReference, CustomAttributeValues};
pub use ropey::Rope;
use tokio::sync::{mpsc::unbounded_channel, mpsc::UnboundedSender};
use winit::event_loop::EventLoopProxy;

use crate::{RopeEditor, TextEditor};

/// Events emitted to the [`UseEditable`].
pub enum EditableEvent {
    Click,
    MouseOver(Rc<MouseData>, usize),
    MouseDown(Rc<MouseData>, usize),
}

/// How the editable content must behave.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum EditableMode {
    /// Multiple editors of only one line.
    ///
    /// Useful for textarea-like editors that need more customization than a simple paragraph for example.
    SingleLineMultipleEditors,
    /// One editor of multiple lines.
    ///
    /// A paragraph for example.
    MultipleLinesSingleEditor,
}

pub type KeypressNotifier = UnboundedSender<Rc<KeyboardData>>;
pub type ClickNotifier = UnboundedSender<EditableEvent>;
pub type EditorState = UseState<RopeEditor>;

/// Manage an editable content.
#[derive(Clone)]
pub struct UseEditable {
    pub editor: EditorState,
    pub keypress_notifier: KeypressNotifier,
    pub click_notifier: ClickNotifier,
    pub cursor_reference: CursorReference,
}

impl UseEditable {
    /// Reference to the editor.
    pub fn editor(&self) -> &EditorState {
        &self.editor
    }

    /// Reference to the Keypress notifier.
    pub fn keypress_notifier(&self) -> &KeypressNotifier {
        &self.keypress_notifier
    }

    /// Reference to the click notifier.
    pub fn click_notifier(&self) -> &ClickNotifier {
        &self.click_notifier
    }

    /// Create a cursor attribute.
    pub fn cursor_attr<'a, T>(&self, cx: Scope<'a, T>) -> AttributeValue<'a> {
        cx.any_value(CustomAttributeValues::CursorReference(
            self.cursor_reference.clone(),
        ))
    }

    /// Create a highlights attribute.
    pub fn highlights_attr<'a, T>(&self, cx: Scope<'a, T>, editor_id: usize) -> AttributeValue<'a> {
        cx.any_value(CustomAttributeValues::TextHighlights(
            self.editor
                .get()
                .highlights(editor_id)
                .map(|v| vec![v])
                .unwrap_or_default(),
        ))
    }
}

/// Create a virtual text editor with it's own cursor and rope.
pub fn use_editable(
    cx: &ScopeState,
    initializer: impl Fn() -> String,
    mode: EditableMode,
) -> UseEditable {
    // Hold the text editor
    let text_editor = use_state(cx, || RopeEditor::from_string(initializer(), mode));

    let cursor_channels = cx.use_hook(|| {
        let (tx, rx) = unbounded_channel::<CursorLayoutResponse>();
        (tx, Some(rx))
    });

    // Cursor reference passed to the layout engine
    let cursor_reference = cx.use_hook(|| CursorReference {
        agent: cursor_channels.0.clone(),
        cursor_position: Arc::new(Mutex::new(None)),
        id: Arc::new(Mutex::new(None)),
        cursor_selections: Arc::new(Mutex::new(None)),
    });

    // Move cursor with clicks
    let click_channel = cx.use_hook(|| {
        let (tx, rx) = unbounded_channel::<EditableEvent>();
        (tx, Some(rx))
    });

    // Write into the text
    let keypress_channel = cx.use_hook(|| {
        let (tx, rx) = unbounded_channel::<Rc<KeyboardData>>();
        (tx, Some(rx))
    });

    let use_editable = UseEditable {
        editor: text_editor.clone(),
        keypress_notifier: keypress_channel.0.clone(),
        click_notifier: click_channel.0.clone(),
        cursor_reference: cursor_reference.clone(),
    };

    // Listen for click events and pass them to the layout engine
    use_effect(cx, (), {
        to_owned![cursor_reference];
        move |_| {
            let editor = text_editor.clone();
            let rx = click_channel.1.take();
            let event_loop_proxy = cx.consume_context::<EventLoopProxy<EventMessage>>();
            async move {
                let mut rx = rx.unwrap();
                let mut current_dragging = None;

                while let Some(edit_event) = rx.recv().await {
                    match &edit_event {
                        EditableEvent::MouseDown(e, id) => {
                            let coords = e.get_element_coordinates();
                            current_dragging = Some(coords);

                            cursor_reference.set_id(Some(*id));
                            cursor_reference
                                .set_cursor_position(Some((coords.x as f32, coords.y as f32)));

                            editor.with_mut(|text_editor| {
                                text_editor.unhighlight();
                            });
                        }
                        EditableEvent::MouseOver(e, id) => {
                            if let Some(current_dragging) = current_dragging {
                                let coords = e.get_element_coordinates();

                                cursor_reference.set_id(Some(*id));
                                cursor_reference.set_cursor_selections(Some((
                                    current_dragging.to_usize().to_tuple(),
                                    coords.to_usize().to_tuple(),
                                )));
                            }
                        }
                        EditableEvent::Click => {
                            current_dragging = None;
                        }
                    }

                    if current_dragging.is_some() {
                        if let Some(event_loop_proxy) = &event_loop_proxy {
                            event_loop_proxy
                                .send_event(EventMessage::RequestRelayout)
                                .unwrap();
                        }
                    }
                }
            }
        }
    });

    // Listen for new calculations from the layout engine
    use_effect(cx, (), move |_| {
        let cursor_reference = cursor_reference.clone();
        let cursor_receiver = cursor_channels.1.take();
        let editor = text_editor.clone();

        async move {
            let mut cursor_receiver = cursor_receiver.unwrap();

            while let Some(message) = cursor_receiver.recv().await {
                match message {
                    // Update the cursor position calculated by the layout
                    CursorLayoutResponse::CursorPosition { position, id } => {
                        let text_editor = editor.current();

                        let new_cursor_row = match mode {
                            EditableMode::MultipleLinesSingleEditor => {
                                text_editor.char_to_line(position)
                            }
                            EditableMode::SingleLineMultipleEditors => id,
                        };

                        let new_cursor_col = match mode {
                            EditableMode::MultipleLinesSingleEditor => {
                                position - text_editor.line_to_char(new_cursor_row)
                            }
                            EditableMode::SingleLineMultipleEditors => position,
                        };

                        let new_current_line = text_editor.line(new_cursor_row).unwrap();

                        // Use the line length as new column if the clicked column surpases the length
                        let new_cursor = if new_cursor_col >= new_current_line.len_chars() {
                            (new_current_line.len_chars(), new_cursor_row)
                        } else {
                            (new_cursor_col, new_cursor_row)
                        };

                        // Only update if it's actually different
                        if text_editor.cursor().as_tuple() != new_cursor {
                            editor.with_mut(|text_editor| {
                                text_editor.cursor_mut().set_col(new_cursor.0);
                                text_editor.cursor_mut().set_row(new_cursor.1);
                                text_editor.unhighlight();
                            })
                        }
                    }
                    // Update the text selections calculated by the layout
                    CursorLayoutResponse::TextSelection { from, to, id } => {
                        editor.with_mut(|text_editor| {
                            text_editor.highlight_text(from, to, id);
                        });
                    }
                }
                // Remove the current calcutions so the layout engine doesn't try to calculate again
                cursor_reference.set_cursor_position(None);
                cursor_reference.set_cursor_selections(None);
            }
        }
    });

    // Listen for keypresses
    use_effect(cx, (), move |_| {
        let rx = keypress_channel.1.take();
        let text_editor = text_editor.clone();
        async move {
            let mut rx = rx.unwrap();

            while let Some(pressed_key) = rx.recv().await {
                text_editor.with_mut(|text_editor| {
                    text_editor.process_key(
                        &pressed_key.key,
                        &pressed_key.code,
                        &pressed_key.modifiers,
                    );
                });
            }
        }
    });

    use_editable
}
