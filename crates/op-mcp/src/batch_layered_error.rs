//! Typed failures for the layered design workflow (`batch_layered.rs`) —
//! `design_skeleton`'s root/section assembly and `design_content`'s children
//! parser.
//!
//! Style follows `ProgramError`: a plain enum plus a hand-written `Display`,
//! no `thiserror` and no new dependency. Each variant's `Display` reproduces
//! the exact sentence the stringly-typed builders produced, because those
//! sentences ship verbatim to the model as the `design_skeleton` /
//! `design_content` `InvalidArgument` payload.
//!
//! What the enum buys over `String` is the CLASSIFICATION plus the section
//! index: a caller can tell "the model's `rootFrame` is wrong" from "section
//! N is wrong" from "the whole `children` payload is not JSON" without
//! re-parsing prose.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LayeredError {
    /// `rootFrame` is not a JSON object.
    RootFrameNotObject,
    /// `rootFrame` omits `width` / `height`.
    RootFrameMissingSize,
    /// A desktop horizontal root flattens ordinary page sections into
    /// several equal-width columns instead of nesting them in a Main frame.
    FlattenedDesktopRoot,
    /// A `sections[i]` entry is not a JSON object.
    SectionNotObject { index: usize },
    /// A `sections[i]` entry has no non-blank `name`.
    SectionMissingName { index: usize },
    /// The assembled root/section tree did not deserialize into `PenNode`s.
    InvalidSkeletonNodes(String),
    /// `children` is not parseable JSON.
    ChildrenNotJson(String),
    /// `children` parsed but is not a JSON array.
    ChildrenNotArray,
    /// A `children` entry did not deserialize into a `PenNode`.
    InvalidChildNodes(String),
}

impl fmt::Display for LayeredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayeredError::RootFrameNotObject => f.write_str("rootFrame must be a JSON object"),
            LayeredError::RootFrameMissingSize => {
                f.write_str("rootFrame must contain width and height")
            }
            LayeredError::FlattenedDesktopRoot => f.write_str(
                "desktop horizontal root has three or more fill_container sections. Use structural shell zones instead: normally a fixed 240-280px Sidebar plus one vertical fill_container Main, then nest page sections inside Main",
            ),
            LayeredError::SectionNotObject { index } => {
                write!(f, "sections[{index}] must be a JSON object")
            }
            LayeredError::SectionMissingName { index } => {
                write!(f, "sections[{index}].name is required")
            }
            LayeredError::InvalidSkeletonNodes(detail) => {
                write!(
                    f,
                    "rootFrame/sections must form valid PenNode objects: {detail}"
                )
            }
            LayeredError::ChildrenNotJson(detail) => {
                write!(f, "children must be a JSON array: {detail}")
            }
            LayeredError::ChildrenNotArray => f.write_str("children must be a JSON array"),
            LayeredError::InvalidChildNodes(detail) => {
                write!(f, "children must contain valid PenNode objects: {detail}")
            }
        }
    }
}

impl std::error::Error for LayeredError {}
