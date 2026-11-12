import { useEffect, useRef, useState } from "react";
import { useEngineApi } from "@/api/context";
import type { TrafficSegment } from "@/api/types";
import { toEngineError, toUserMessage } from "@/lib/errors";
import { decodeBase64 } from "./trafficText";

/** Each direction keeps this many of the newest segments; older ones leave it (the engine's bound). */
export const MAX_SEGMENTS = 1000;

export type Pane = TrafficSegment["direction"];

export interface TrafficEntry {
  /** Unique within this panel, even when the engine restarts its sequence numbers. */
  key: number;
  segment: TrafficSegment;
  bytes: Uint8Array;
}

interface PaneFeed {
  entries: readonly TrafficEntry[];
  /** Segments received since the last Clear, including those that have left the pane. */
  received: number;
}

export interface Feed {
  a_to_b: PaneFeed;
  b_to_a: PaneFeed;
}

const EMPTY_PANE: PaneFeed = { entries: [], received: 0 };
const EMPTY: Feed = { a_to_b: EMPTY_PANE, b_to_a: EMPTY_PANE };

function append(pane: PaneFeed, added: readonly TrafficEntry[]): PaneFeed {
  if (added.length === 0) return pane;
  return {
    entries: [...pane.entries, ...added].slice(-MAX_SEGMENTS),
    received: pane.received + added.length,
  };
}

/**
 * The live transcript of one channel. It is opt-in: the engine is subscribed only while
 * `isOn`, and unsubscribed when it is turned off or the screen goes away. Segments stay in
 * memory only, bounded per direction. Freezing stops the display, not the capture: the feed
 * keeps filling, and unfreezing shows the newest segments. A subscription the engine refuses
 * (another client is viewing) leaves the transcript off and keeps the engine's words in
 * `rejection`.
 */
export function useTrafficFeed(channelId: string) {
  const api = useEngineApi();
  const [isOn, setIsOn] = useState(false);
  const [feed, setFeed] = useState<Feed>(EMPTY);
  const [frozen, setFrozen] = useState<Feed | null>(null);
  const [rejection, setRejection] = useState<string | null>(null);
  const nextKey = useRef(0);

  // Synchronizes with the engine's subscription, which lives outside React.
  useEffect(() => {
    if (!isOn) return;
    try {
      return api.subscribeTraffic(
        channelId,
        (segments) => {
          if (segments.length === 0) return;
          const added = segments.map((segment) => {
            nextKey.current += 1;
            return { key: nextKey.current, segment, bytes: decodeBase64(segment.base64) };
          });
          setFeed((current) => ({
            a_to_b: append(
              current.a_to_b,
              added.filter((e) => e.segment.direction === "a_to_b"),
            ),
            b_to_a: append(
              current.b_to_a,
              added.filter((e) => e.segment.direction === "b_to_a"),
            ),
          }));
        },
        (error) => {
          setRejection(toUserMessage(error));
          setIsOn(false);
        },
      );
    } catch (error) {
      setRejection(toUserMessage(toEngineError(error)));
      setIsOn(false);
    }
  }, [api, channelId, isOn]);

  return {
    isOn,
    start: () => {
      setRejection(null);
      setIsOn(true);
    },
    stop: () => setIsOn(false),
    /** What the panes show: the frozen view while frozen, else everything captured. */
    shown: frozen ?? feed,
    isFrozen: frozen !== null,
    freeze: () => setFrozen(feed),
    unfreeze: () => setFrozen(null),
    /** Segments captured since the display froze. */
    heldBack:
      frozen === null
        ? 0
        : feed.a_to_b.received -
          frozen.a_to_b.received +
          feed.b_to_a.received -
          frozen.b_to_a.received,
    clear: () => {
      setFeed(EMPTY);
      setFrozen((current) => (current === null ? null : EMPTY));
    },
    rejection,
  };
}
