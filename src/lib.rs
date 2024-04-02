//! # debugbunny
//!
//! Execute HTTP-requests and commands at (un)scheduled intervals and collect
//! their output in log-compatible format.
//!
//! ## Scrape Target
//!
//! A scrape target is either a URL or a command. When _calling_ a scrape
//! target, the underlying action (HTTP-request, command execution ...) is being
//! executed to collect the output of that scrape target.
//!
//! Generally, each scrape target is called at a fixed interval. Executing the
//! call should not delay the schedule, unless the execution of the call takes
//! longer than the interval.
//!
//! It is possible to call to a ScrapeTarget at an _unscheduled_, random point
//! in time. Such a call resets the schedule. However, all calls to a scrape
//! target are always synchronized and *never* overlap; i.e., a currently
//! running call will delay new calls.
//!
//! ## Architecture
//!
//! +--------------------------------------------+
//! | Scheduled Target     | Unscheduled Target  |
//! +--------------------------------------------+        
//! |                Scrape Target               |
//! +--------------------------------------------+        
//! |             Timeout Enforcement            |
//! +--------------------------------------------+
//! |              HTTP | Command | ...          |
//! +--------------------------------------------+

pub mod chunks;
pub mod command;
pub mod config;
pub mod http;
pub mod result_processor;
pub mod scrape_target;
pub mod target_collection;
