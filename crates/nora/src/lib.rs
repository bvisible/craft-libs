#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// //// Neoffice — added file (no upstream equivalent): the Nora client core shared by the Neoffice editors.

//! Client core for Nora, the Neoffice assistant, usable by every Craft app.
//!
//! Nora itself (the agent, its memory and its tools) stays on the Neoffice server. This crate is
//! what an editor needs to talk to it: the wire protocol, the state of a conversation, and the
//! actions Nora asks for inside the open document.
//!
//! It is **sans I/O**. It never opens a socket and never reads a clock: the host (NeoDesk, the
//! Drive page, a native shell) performs the HTTP requests with its own session and credentials and
//! feeds the answers back, and the app executes document actions through its own control channel.
//! The same code therefore runs in a browser (wasm) and natively, and is tested without a network.
//!
//! ```
//! use craft_nora::{AppKind, DocumentInfo, Input, NoraSession, Output, PageContext, SessionConfig};
//!
//! let ctx = PageContext::editor(AppKind::Word, DocumentInfo::named("devis.docx"));
//! let mut nora = NoraSession::new(SessionConfig::default(), ctx);
//! let out = nora.handle(Input::Send("Rewrite the first paragraph".into()));
//! assert!(out.iter().any(|o| matches!(o, Output::Http(_))));
//! ```
//!
//! The wire protocol is the one Nora already speaks to the Neoffice desk and apps: the
//! `nora.api.chat.*` methods (`start_or_resume_thread`, `post_message`, `get_messages`,
//! `get_browser_actions`) and `nora.api.page_observation.receive_observation`. An editor is one
//! canvas, so Nora asks for it through its own family of actions, `doc_*` ([`DocActionKind`]).
//!
//! A [`Proposal`] is what Nora prepared for the person to send (NeoMail's ready replies): replies with fields to fill
//! where she could not know, each empty field a visible marker until it is filled ([`proposal`]).
//!
//! With the `ai-core` feature, the errors and the turns also speak the vocabulary of
//! `craft-ai-core` (`NoraError::kind`, `NoraSession::turn_state`), so that a host shows Nora with its
//! other AI connections.

pub mod context;
pub mod error;
pub mod http;
pub mod links;
pub mod policy;
pub mod proposal;
pub mod protocol;
pub mod reply;
pub mod session;

pub use context::{AppKind, DocumentInfo, PageContext, Selection};
pub use error::NoraError;
pub use http::{HttpRequest, HttpResponse, RequestId};
pub use policy::{CommandPolicy, Decision};
pub use proposal::{Field, FieldKind, Piece, Proposal, Ready, Reply, StyleState, parse_ready};
pub use protocol::{Card, CardChoice, ChatMessage, CommandCall, DocAction, DocActionKind, RemoteAction, Role};
pub use reply::{Segment, instance_path, segments};
pub use session::{Confirmation, Correction, DocumentCall, Input, NoraSession, Output, SessionConfig, ShownCard};
