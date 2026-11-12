import {
  QueryClient,
  type QueryKey,
  type UseMutationResult,
  type UseQueryOptions,
  type UseQueryResult,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useEffect } from "react";
import { toEngineError } from "@/lib/errors";
import { useEngineApi } from "./context";
import type { EngineApi } from "./engine";
import type {
  ChannelDetail,
  ConnectionStatus,
  EndpointDetail,
  EngineError,
  Snapshot,
} from "./types";

/** The app's cache: engine errors are shown with a retry button, not retried silently. */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, refetchOnWindowFocus: false, staleTime: 5_000 },
      mutations: { retry: false },
    },
  });
}

type QueryExtras<Data> = Pick<
  UseQueryOptions<Data, EngineError>,
  "enabled" | "refetchInterval" | "staleTime" | "placeholderData"
>;

/**
 * One `EngineApi` read as a query. The key starts with the method name, then its arguments
 * (`["getChannel", id]`), so a mutation can invalidate everything one method fed.
 */
export function useEngineQuery<Data>(
  key: QueryKey,
  read: (api: EngineApi) => Promise<Data>,
  extras: QueryExtras<Data> = {},
): UseQueryResult<Data, EngineError> {
  const api = useEngineApi();
  return useQuery<Data, EngineError>({
    queryKey: key,
    queryFn: async () => {
      try {
        return await read(api);
      } catch (error) {
        throw toEngineError(error);
      }
    },
    ...extras,
  });
}

/**
 * One `EngineApi` command. On success the queries whose key starts with any of
 * `invalidates` (method names) are refetched; the snapshot is always among them.
 */
export function useEngineMutation<Input, Output = void>(
  run: (api: EngineApi, input: Input) => Promise<Output>,
  { invalidates = [] }: { invalidates?: readonly string[] } = {},
): UseMutationResult<Output, EngineError, Input> {
  const api = useEngineApi();
  const client = useQueryClient();
  return useMutation<Output, EngineError, Input>({
    mutationFn: async (input) => {
      try {
        return await run(api, input);
      } catch (error) {
        throw toEngineError(error);
      }
    },
    onSuccess: async () => {
      await Promise.all(
        ["getSnapshot", ...invalidates].map((method) =>
          client.invalidateQueries({ queryKey: [method] }),
        ),
      );
    },
  });
}

const SNAPSHOT_KEY = ["getSnapshot"] as const;

/**
 * Mount once, inside the shell: keeps the snapshot query current from the engine's
 * subscription (every second and on change), so pages just read `useSnapshot()`.
 */
export function useSnapshotSync(): void {
  const api = useEngineApi();
  const client = useQueryClient();
  useEffect(
    () => api.subscribeSnapshots((snapshot) => client.setQueryData(SNAPSHOT_KEY, snapshot)),
    [api, client],
  );
}

/** The latest snapshot of the node, channels, endpoints and open alerts. */
export function useSnapshot(): UseQueryResult<Snapshot, EngineError> {
  return useEngineQuery(SNAPSHOT_KEY, (api) => api.getSnapshot(), { staleTime: Infinity });
}

export function useChannel(id: string): UseQueryResult<ChannelDetail, EngineError> {
  return useEngineQuery(["getChannel", id], (api) => api.getChannel(id), {
    refetchInterval: 5_000,
  });
}

export function useEndpoint(id: string): UseQueryResult<EndpointDetail, EngineError> {
  return useEngineQuery(["getEndpoint", id], (api) => api.getEndpoint(id), {
    refetchInterval: 5_000,
  });
}

const CONNECTION_KEY = ["getConnection"] as const;

/**
 * Mount once, above the gate: keeps the connection query current from the engine
 * connection's own state events (connected, disconnected, reconnecting).
 */
export function useConnectionSync(): void {
  const api = useEngineApi();
  const client = useQueryClient();
  useEffect(
    () => api.subscribeConnection((status) => client.setQueryData(CONNECTION_KEY, status)),
    [api, client],
  );
}

/** Whether, and where, the console is attached to an engine. */
export function useConnection(): UseQueryResult<ConnectionStatus, EngineError> {
  return useEngineQuery(CONNECTION_KEY, (api) => api.getConnection(), { staleTime: Infinity });
}
