### line_height

Specify the height of the lines of the text.

### Example

```rust, no_run
# use freya::prelude::*;
fn app(cx: Scope) -> Element {
    render!(
        label {
            line_height: "3",
            "Hello, World! \n Hello, again!"
        }
    )
}
```
