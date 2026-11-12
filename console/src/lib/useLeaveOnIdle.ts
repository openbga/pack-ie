import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { useNavigate } from "react-router";
import { useOnIdle } from "@/lib/idle";

/**
 * For a screen that is itself content (a message record): when the console goes idle, leaves
 * for `to`, replacing this screen in the history so Back does not return to it, and removes
 * the cached reads of the engine method `forgetMethod` (such as `getMessage`). The cache is
 * cleared once the screen is gone, so nothing refetches what was just dropped.
 */
export function useLeaveOnIdle(to: string, forgetMethod: string): void {
  const navigate = useNavigate();
  const client = useQueryClient();
  const left = useRef(false);
  useOnIdle(() => {
    left.current = true;
    void navigate(to, { replace: true });
  });
  useEffect(
    // Runs when the screen unmounts; `left` is still false in development's first, simulated one.
    () => () => {
      if (left.current) client.removeQueries({ queryKey: [forgetMethod] });
    },
    [client, forgetMethod],
  );
}
