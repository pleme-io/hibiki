//! GPU rendering module -- wgpu pipeline for the hibiki player UI.
//!
//! Uses garasu for GPU context, text rendering, and shader pipeline.
//! Uses madori for the application shell (event loop, render loop).
//! Uses egaku for widget state machines (lists, tabs, focus).
//!
//! Layout:
//! ```text
//! +-------+------------------+--------+
//! |       |                  |        |
//! | Lib   |   Now Playing    | Queue  |
//! | Panel |   (album art,    | Panel  |
//! |       |    track info,   |        |
//! |       |    visualizer)   |        |
//! |       |                  |        |
//! +-------+------------------+--------+
//! | [status bar: track / position / volume / shuffle / repeat] |
//! +------------------------------------------------------------+
//! ```

mod player;
mod state;

pub use player::HibikiRenderer;
pub use state::Panel;
