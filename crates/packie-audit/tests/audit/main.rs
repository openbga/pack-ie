#![cfg(test)]
//! Audit log behavior: recording, review, tamper detection, fail-closed writes, sealing,
//! retention, anchors and the cost of recording.

mod availability;
mod common;
mod lifecycle;
mod performance;
mod review;
mod tamper;
