//! `reddit_sources`: the only code that touches the network. One source exists, the Arctic Shift archive.
//! Everything goes through `Transport`, so every test replays recorded responses.

pub mod arctic;
pub mod transport;
