//! Framing: a 4-byte big-endian length, then that many bytes of UTF-8 JSON.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

/// Largest accepted frame (16 MiB); larger lengths close the connection.
pub const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// A frame could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// The connection failed.
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),
    /// The peer announced a frame larger than [`MAX_FRAME`].
    #[error("frame of {0} bytes exceeds the {MAX_FRAME}-byte limit")]
    TooLarge(u64),
    /// The frame is not the expected JSON.
    #[error("malformed frame: {0}")]
    Json(#[from] serde_json::Error),
}

/// Reads one frame's bytes; `None` on a clean end of stream between frames.
///
/// # Errors
///
/// [`FrameError::Io`] when the connection fails; [`FrameError::TooLarge`] when the peer
/// announces more than [`MAX_FRAME`] bytes.
pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Vec<u8>>, FrameError> {
    let mut len = [0_u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len);
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(u64::from(len)));
    }
    let mut body = vec![0_u8; usize::try_from(len).unwrap_or(usize::MAX)];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

/// Writes one frame and flushes.
///
/// # Errors
///
/// [`FrameError::TooLarge`] when `body` exceeds [`MAX_FRAME`]; [`FrameError::Io`] when the
/// connection fails.
pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    body: &[u8],
) -> Result<(), FrameError> {
    let len = u32::try_from(body.len())
        .ok()
        .filter(|len| *len <= MAX_FRAME)
        .ok_or(FrameError::TooLarge(
            u64::try_from(body.len()).unwrap_or(u64::MAX),
        ))?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(body).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one frame as `T`; `None` on a clean end of stream.
///
/// # Errors
///
/// As for [`read_frame`]; [`FrameError::Json`] when the frame is not a `T`.
pub async fn read_json<T: DeserializeOwned, R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<T>, FrameError> {
    match read_frame(reader).await? {
        Some(body) => Ok(Some(serde_json::from_slice(&body)?)),
        None => Ok(None),
    }
}

/// Writes `value` as one frame.
///
/// # Errors
///
/// As for [`write_frame`]; [`FrameError::Json`] when `value` cannot be serialized.
pub async fn write_json<T: Serialize, W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &T,
) -> Result<(), FrameError> {
    write_frame(writer, &serde_json::to_vec(value)?).await
}

/// Writes every value of `outgoing` as a frame until the channel closes (`None`) or a write
/// fails (that error).
pub(crate) async fn write_all<T: Serialize, W: AsyncWrite + Unpin>(
    writer: &mut W,
    outgoing: &mut mpsc::Receiver<T>,
) -> Option<FrameError> {
    while let Some(value) = outgoing.recv().await {
        if let Err(error) = write_json(writer, &value).await {
            return Some(error);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip_and_eof_is_clean() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_json(&mut a, &serde_json::json!({"x": 1}))
            .await
            .unwrap();
        write_frame(&mut a, b"[]").await.unwrap();
        drop(a);
        let first: serde_json::Value = read_json(&mut b).await.unwrap().unwrap();
        assert_eq!(first["x"], 1);
        assert_eq!(read_frame(&mut b).await.unwrap().unwrap(), b"[]");
        assert!(read_frame(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn oversized_frames_are_refused() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&(MAX_FRAME + 1).to_be_bytes()).await.unwrap();
        assert!(matches!(
            read_frame(&mut b).await,
            Err(FrameError::TooLarge(_))
        ));
    }
}
