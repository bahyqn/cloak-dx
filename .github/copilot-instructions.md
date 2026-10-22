# GitHub Copilot Instructions for cloack-dx

- **Tech Stack**: Rust, Dioxus (Desktop), Tokio, SQLite.
- **Target OS**: Debian GNOME.
- **Core Architectural Rules**:
    1. **Modularization (No Monolithic Files)**: Writing all code in a single file (e.g. monolithic `main.rs`) is strictly forbidden. Complex pages and views must be decomposed into independent components and assembled modularly.
    2. **Theme Isolation**: Themes (colors, style tokens, design constants) must be extracted into a dedicated module/file. Hardcoding theme values inside UI components is prohibited.
    3. **UI Decoupling**: UI must strictly follow the HTML/CSS prototypes in `ui-mockups/`.
    4. **Subscription Conversion**: Zero local conversion generation; must rely exclusively on an external imported Rust crate.
    5. **Logging Isolation**: Application logs (with categories/filters) and Proxy Core logs (with ring-buffer limit) must be completely separate.
    6. **Independent Benchmark**: Node testing and analytics engines must operate asynchronously and independently of UI threads.
