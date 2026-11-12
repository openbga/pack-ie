use thiserror::Error;

/// Protocol failures; no variant contains received content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AuError {
    /// The configuration describes an unsupported or invalid profile.
    #[error("invalid AU configuration")]
    Configuration,
    /// A frame exceeds the configured wire limit.
    #[error("AU frame exceeds configured limit")]
    FrameTooLong,
    /// Start, end or framing bytes are not as configured.
    #[error("invalid AU framing")]
    Framing,
    /// The block check character does not match the frame contents.
    #[error("AU block check character does not match")]
    Checksum,
    /// The text contains a byte outside printable ASCII.
    #[error("unsupported AU text character")]
    Character,
    /// The two-byte message classification is unknown.
    #[error("unknown AU message classification")]
    Classification,
    /// A fixed-width field has an invalid value.
    #[error("invalid AU fixed-width field")]
    Field,
    /// The message length does not match the configured layout.
    #[error("invalid AU message length")]
    Length,
    /// The block identification is neither 0-9 nor E.
    #[error("invalid AU block identification")]
    BlockId,
    /// The terminator (ETB or text end) does not match the block identification.
    #[error("AU block terminator does not match block identification")]
    BlockTerminator,
    /// The sample header changed within a blocked message.
    #[error("AU sample header changed within a blocked message")]
    HeaderConflict,
    /// A numeric block was skipped or reordered.
    #[error("AU block sequence skipped or reordered")]
    BlockSequence,
    /// A retransmitted block differs from the previously received one.
    #[error("AU retransmission conflicts with the previous block")]
    BlockConflict,
    /// A sample ended before its final block.
    #[error("AU sample ends before its final block")]
    IncompleteSample,
    /// The message needs more than ten intermediate blocks.
    #[error("AU message requires too many blocks")]
    TooManyBlocks,
}
