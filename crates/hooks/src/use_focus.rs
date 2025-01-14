use accesskit::NodeId as AccessibilityId;
use dioxus_core::{AttributeValue, Scope, ScopeState};
use dioxus_hooks::{use_context, use_shared_state, UseSharedState};
use freya_core::accessibility::ACCESSIBILITY_ROOT_ID;
use freya_core::navigation_mode::{NavigationMode, NavigatorState};
use freya_elements::events::{keyboard::Code, KeyboardEvent};
use freya_node_state::CustomAttributeValues;

use crate::AccessibilityIdCounter;

/// Manage the focus operations of given Node
#[derive(Clone)]
pub struct UseFocus {
    id: AccessibilityId,
    focused_id: UseSharedState<AccessibilityId>,
    navigation_state: NavigatorState,
}

impl UseFocus {
    /// Focus this node
    pub fn focus(&self) {
        *self.focused_id.write() = self.id
    }

    /// Get the node focus ID
    pub fn id(&self) -> AccessibilityId {
        self.id
    }

    /// Create a node focus ID attribute
    pub fn attribute<'b, T>(&self, cx: Scope<'b, T>) -> AttributeValue<'b> {
        cx.any_value(CustomAttributeValues::AccessibilityId(self.id))
    }

    /// Check if this node is currently focused
    pub fn is_focused(&self) -> bool {
        self.id == *self.focused_id.read()
    }

    /// Check if this node is currently selected
    pub fn is_selected(&self) -> bool {
        self.is_focused() && self.navigation_state.get() == NavigationMode::Keyboard
    }

    /// Unfocus the currently focused node.
    pub fn unfocus(&self) {
        *self.focused_id.write() = ACCESSIBILITY_ROOT_ID;
    }

    /// Validate keydown event
    pub fn validate_keydown(&self, e: KeyboardEvent) -> bool {
        e.data.code == Code::Enter && self.is_selected()
    }
}

/// Create a focus manager for a node.
pub fn use_focus(cx: &ScopeState) -> &UseFocus {
    let accessibility_id_counter = use_context::<AccessibilityIdCounter>(cx).unwrap();
    let focused_id = use_shared_state::<AccessibilityId>(cx).unwrap();

    cx.use_hook(|| {
        let mut counter = accessibility_id_counter.borrow_mut();
        *counter += 1;
        let id = AccessibilityId(*counter);

        let navigation_state = cx
            .consume_context::<NavigatorState>()
            .expect("This is not expected, and likely a bug. Please, report it.");

        UseFocus {
            id,
            focused_id: focused_id.clone(),
            navigation_state,
        }
    })
}

#[cfg(test)]
mod test {
    use crate::use_focus;
    use freya::prelude::*;
    use freya_testing::{
        events::pointer::MouseButton, launch_test_with_config, FreyaEvent, TestingConfig,
    };

    #[tokio::test]
    pub async fn track_focus() {
        #[allow(non_snake_case)]
        fn OherChild(cx: Scope) -> Element {
            let focus_manager = use_focus(cx);

            render!(
                rect {
                    width: "100%",
                    height: "50%",
                    onclick: move |_| focus_manager.focus(),
                    "{focus_manager.is_focused()}"
                }
            )
        }

        fn use_focus_app(cx: Scope) -> Element {
            render!(
                rect {
                    width: "100%",
                    height: "100%",
                    OherChild {},
                    OherChild {}
                }
            )
        }

        let mut utils = launch_test_with_config(
            use_focus_app,
            *TestingConfig::default().with_size((100.0, 100.0).into()),
        );

        // Initial state
        utils.wait_for_update().await;
        let root = utils.root().get(0);
        assert_eq!(root.get(0).get(0).text(), Some("false"));
        assert_eq!(root.get(1).get(0).text(), Some("false"));

        // Click on the first rect
        utils.push_event(FreyaEvent::Mouse {
            name: "click".to_string(),
            cursor: (5.0, 5.0).into(),
            button: Some(MouseButton::Left),
        });

        // First rect is now focused
        utils.wait_for_update().await;
        assert_eq!(root.get(0).get(0).text(), Some("true"));
        assert_eq!(root.get(1).get(0).text(), Some("false"));

        // Click on the second rect
        utils.push_event(FreyaEvent::Mouse {
            name: "click".to_string(),
            cursor: (5.0, 75.0).into(),
            button: Some(MouseButton::Left),
        });

        // Second rect is now focused
        utils.wait_for_update().await;
        assert_eq!(root.get(0).get(0).text(), Some("false"));
        assert_eq!(root.get(1).get(0).text(), Some("true"));
    }
}
