// Allow PyO3-specific false positive warnings
#![allow(clippy::useless_conversion)]

//! KohakuTUI - terminal UI engine driven from Python through a retained node-tree protocol
//!
//! Modules, in data-flow order:
//! - proto: Ops (down), Events (up), the dynamic value type and wire framing
//! - model: the retained document (regions, nodes, seal and flush state)
//! - text: Markdown rendering, streaming hold-back, width-aware wrapping
//! - widgets: the component registry and every component
//! - view: focus, key routing, overlays, layout, theme
//! - term: TTY ownership, inline viewport, history insertion, alternate screen
//! - runtime: the render thread, input thread and stdio capture
//! - transport: the Bus, framed streams, Host and Viewer
//!
//! With the `python` feature this crate builds the `kohakutui._rs` extension module.

pub mod model;
pub mod proto;
pub mod runtime;
pub mod term;
pub mod text;
pub mod transport;
pub mod view;
pub mod widgets;

/// Crate version, the single source of the package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "python")]
mod py {
    use pyo3::prelude::*;

    /// Version of the compiled extension.
    #[pyfunction]
    fn version() -> &'static str {
        crate::VERSION
    }

    #[pymodule]
    fn _rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
        m.add_function(wrap_pyfunction!(version, m)?)?;
        m.add("PROTOCOL", crate::proto::PROTOCOL)?;
        Ok(())
    }
}
