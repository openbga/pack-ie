use crate::{Redacted, TriTest};

/// Numeric intermediate blocks are 0-9; E is the sole final block (p. 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockId {
    /// Intermediate block number 0-9.
    Intermediate(u8),
    /// The final block, `E`.
    Final,
}

/// Session boundaries are distinct from per-sample requests and results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    /// `RB`: the analyzer starts a query session.
    QueryStart,
    /// `RE`: the analyzer ends a query session.
    QueryEnd,
    /// `DB`: the analyzer starts sending results.
    ResultsStart,
    /// `DE`: the analyzer stops sending results.
    ResultsEnd,
    /// `SE`: the host stops a requisition session.
    TriStop,
}

/// The classification is preserved without promoting nonpatient results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    /// `D `: normal patient result.
    Normal,
    /// `DH`: rerun of a patient sample.
    Rerun,
    /// `d `: stat patient result.
    Stat,
    /// `dH`: stat rerun.
    StatRerun,
    /// `DR`: reagent blank.
    ReagentBlank,
    /// `DA`: calibration.
    Calibration,
    /// `DQ`: quality control.
    QualityControl,
}
impl ResultKind {
    pub(crate) fn normal(self) -> bool {
        matches!(self, Self::Normal | Self::Stat)
    }
}

/// Each raw demographic field retains original padding and has redacted Debug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Demographics {
    /// Sex byte (empty when not configured).
    pub sex: Redacted<Vec<u8>>,
    /// Three age bytes (empty when not configured).
    pub age: Redacted<Vec<u8>>,
    /// Two month bytes (empty when not configured).
    pub month: Redacted<Vec<u8>>,
    /// Six opaque information fields of configured widths.
    pub information: [Redacted<Vec<u8>>; 6],
}

/// Fixed-width sample header shared across every block of one sample.
/// `reserved` is original sample number for reruns, dummy/control fields for
/// other kinds. QC sample-type is opaque dummy, never interpreted clinically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleHeader {
    /// Two-digit unit number, when enabled and present in the message.
    pub unit_no: Option<Redacted<Vec<u8>>>,
    /// Rack field (empty when the rack width is 0).
    pub rack: Redacted<Vec<u8>>,
    /// Cup field (empty when the rack width is 0).
    pub cup: Redacted<Vec<u8>>,
    /// One-byte sample type.
    pub sample_type: Redacted<Vec<u8>>,
    /// Four-byte analyzer sample number.
    pub sample_no: Redacted<Vec<u8>>,
    /// Sample ID of the configured width.
    pub sample_id: Redacted<Vec<u8>>,
    /// Four reserved bytes (original sample number for reruns; empty in queries).
    pub reserved: Redacted<Vec<u8>>,
    /// Demographics of normal patient messages.
    pub demographics: Option<Demographics>,
}

/// One instrument TRI query; rerun does not imply an authorized order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    /// `RH` rather than `R `.
    pub rerun: bool,
    /// The queried sample header.
    pub header: SampleHeader,
}

/// A result item with exact value/flags; no interpretation of all-9 or LIH data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultTest {
    /// Test code of the configured width (2 or 3 digits).
    pub code: Redacted<Vec<u8>>,
    /// Raw value of the configured width, padding included.
    pub value: Redacted<Vec<u8>>,
    /// Two raw flag bytes.
    pub flags: Redacted<Vec<u8>>,
}

/// A validated result block, including exact classification and repeated header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultBlock {
    /// Message classification.
    pub kind: ResultKind,
    /// The repeated sample header.
    pub header: SampleHeader,
    /// Block identification.
    pub block: BlockId,
    /// Result items carried by this block.
    pub tests: Vec<ResultTest>,
}

/// Parsed host TRI block; methods occur only for reruns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriBlock {
    /// `SH` rather than `S `.
    pub rerun: bool,
    /// The requisition's sample header.
    pub header: SampleHeader,
    /// Block identification.
    pub block: BlockId,
    /// Requested tests carried by this block.
    pub tests: Vec<TriTest>,
}

/// Classified application text; byte provenance remains on the source Frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// A session boundary (`RB`, `RE`, `DB`, `DE`, `SE`).
    Session {
        /// Which boundary.
        kind: SessionKind,
        /// Unit number, when enabled.
        unit_no: Option<Redacted<Vec<u8>>>,
    },
    /// A query for a requisition.
    Query(Query),
    /// One block of a sample's results.
    ResultBlock(ResultBlock),
    /// One block of a host requisition.
    TriBlock(TriBlock),
}

/// Complete result text for one sample after all blocks have been checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleResults {
    /// Message classification shared by every block.
    pub kind: ResultKind,
    /// The sample header shared by every block.
    pub header: SampleHeader,
    /// All result items in block order.
    pub tests: Vec<ResultTest>,
}
