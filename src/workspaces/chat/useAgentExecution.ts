import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { executionApi, executionBusy, type AgentRunEvent, type ExecutionStatus } from "../../lib/execution";
import { authorizationApi, type AuthorizationDecision } from "../../lib/authorization";

export function useAgentExecution(conversationId: number | null, onMessagesChanged: (id: number) => void) {
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [changingMode, setChangingMode] = useState(false);
  const [error, setError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [deciding, setDeciding] = useState<number | null>(null);
  const selection = useRef(conversationId);
  selection.current = conversationId;
  const onMessages = useRef(onMessagesChanged);
  onMessages.current = onMessagesChanged;
  const request = useRef(0);

  const refresh = useCallback(async () => {
    if (conversationId === null || selection.current !== conversationId) return;
    const version = ++request.current;
    try {
      const next = await executionApi.status(conversationId);
      if (selection.current !== conversationId || request.current !== version) return;
      setStatus(next);
      setError(false);
    } catch {
      if (selection.current === conversationId && request.current === version) setError(true);
    }
  }, [conversationId]);

  useEffect(() => {
    setStatus(null);
    setEnabled(false);
    setStarting(false);
    setStopping(false);
    setChangingMode(false);
    setError(false);
    setActionError(false);
    setDeciding(null);
    void refresh();
    let disposed = false;
    const subscription = listen<AgentRunEvent>("agent-run-event", ({ payload }) => {
      const id = payload.type === "cycle" ? payload.event.conversationId : payload.conversationId;
      if (id !== conversationId || disposed) return;
      void refresh();
      if (payload.type !== "cycle") onMessages.current(id);
    }).catch(() => () => {});
    return () => {
      disposed = true;
      request.current += 1;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [conversationId, refresh]);

  const visibleStatus = status?.conversationId === conversationId ? status : null;
  const busy = starting || executionBusy(visibleStatus);
  useEffect(() => {
    if (!busy && !status?.blocker) return;
    const timer = window.setInterval(() => {
      void refresh();
      if (conversationId !== null) onMessages.current(conversationId);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [busy, status?.blocker, conversationId, refresh]);

  async function start(content: string) {
    if (conversationId === null || !status || status.conversationId !== conversationId || error || actionError || busy || status.blocker) throw new Error("AgentBusy");
    const id = conversationId;
    setStarting(true);
    try {
      return await executionApi.start(id, content);
    } finally {
      if (selection.current === id) {
        setStarting(false);
        await refresh();
        onMessages.current(id);
      }
    }
  }

  async function cancel(owner = status?.conversationId, runId = status?.execution?.run.id) {
    if (owner === undefined || runId === undefined || stopping) return false;
    const selected = conversationId;
    setStopping(true);
    try {
      await executionApi.cancel(owner, runId);
      await refresh();
      onMessages.current(owner);
      return true;
    } catch {
      if (selection.current === selected) setActionError(true);
      return false;
    } finally {
      if (selection.current === selected) setStopping(false);
    }
  }

  async function mode(value: ExecutionStatus["mode"]) {
    if (!status || changingMode) return;
    setChangingMode(true);
    try {
      await executionApi.mode(status, value);
      await refresh();
    } catch {
      if (selection.current === conversationId) setActionError(true);
    } finally {
      if (selection.current === conversationId) setChangingMode(false);
    }
  }

  function retry() {
    setActionError(false);
    void refresh();
  }

  async function decide(requestId: number, decision: AuthorizationDecision) {
    if (conversationId === null || deciding !== null || !visibleStatus) return;
    const request = visibleStatus.pendingRequests?.find((request) => request.id === requestId);
    if (!request || (decision.decision === "approve" && (!visibleStatus.continuationAvailable || request.requirements.length === 0 || request.requirements.some((requirement) => !requirement.previewAvailable)))) return;
    const id = conversationId;
    setDeciding(requestId);
    try {
      await authorizationApi.decide(id, requestId, decision);
      await refresh();
    } catch {
      if (selection.current === id) setActionError(true);
    } finally {
      if (selection.current === id) setDeciding(null);
    }
  }

  return { status: visibleStatus, enabled, setEnabled, busy, stopping, changingMode, deciding, error: error || actionError, refresh, retry, start, cancel, mode, decide };
}
