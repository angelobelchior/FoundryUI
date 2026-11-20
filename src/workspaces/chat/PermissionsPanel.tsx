import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { authorizationApi, type AuthorizationGrant } from "../../lib/authorization";
import { instructionApi, type InstructionDiscovery } from "../../lib/instructions";
import { projectControlFocus } from "./projectFocus";

export function PermissionsPanel({ conversationId, onChanged }: { conversationId: number; onChanged: () => void }) {
  const { t } = useTranslation();
  const [grants, setGrants] = useState<AuthorizationGrant[] | null>(null);
  const [projectBound, setProjectBound] = useState<boolean | null>(null);
  const [instructions, setInstructions] = useState<InstructionDiscovery | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [revoking, setRevoking] = useState<number | null>(null);
  const [picking, setPicking] = useState(false);
  const [deciding, setDeciding] = useState<string | null>(null);
  const mounted = useRef(true);
  const generation = useRef(0);
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;

  const refresh = useCallback(async () => {
    const version = ++generation.current;
    try {
      const [revision, grantList, discovery] = await Promise.all([
        authorizationApi.revision(conversationId),
        authorizationApi.grants(conversationId),
        instructionApi.discover(conversationId),
      ]);
      if (!mounted.current || generation.current !== version) return;
      setProjectBound(revision.projectBound);
      setGrants(grantList);
      setInstructions(discovery);
      setError(false);
    } catch {
      if (mounted.current && generation.current === version) setError(true);
    } finally {
      if (mounted.current && generation.current === version) setLoading(false);
    }
  }, [conversationId]);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current += 1; };
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function revoke(grantId: number) {
    if (revoking !== null) return;
    setRevoking(grantId);
    try {
      await authorizationApi.revoke(conversationId, grantId);
      if (!mounted.current) return;
      setActionError(false);
      onChangedRef.current();
      await refresh();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setRevoking(null);
    }
  }

  async function pickRoot() {
    if (picking) return;
    setPicking(true);
    try {
      const granted = await authorizationApi.pickRoot(conversationId);
      if (!mounted.current) return;
      setActionError(false);
      onChangedRef.current();
      if (granted) await refresh();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setPicking(false);
    }
  }

  async function decide(source: NonNullable<InstructionDiscovery["sources"][number]>, approve: boolean) {
    if (deciding !== null || !instructions) return;
    setDeciding(source.identityKey);
    try {
      await instructionApi.decide(instructions, source, approve);
      if (!mounted.current) return;
      setActionError(false);
      onChangedRef.current();
      await refresh();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setDeciding(null);
    }
  }

  const button = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  const statusLabels: Record<string, string> = {
    pending: t("permissions.statusPending"),
    approved: t("permissions.statusApproved"),
    denied: t("permissions.statusDenied"),
  };

  return <div className="space-y-4 text-[14px] [&_p]:max-w-[75ch]">
      <div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="font-medium">{t("permissions.grants")}</h2>
          {projectBound === false ? <button type="button" className={button} disabled={picking} onClick={() => void pickRoot()}>{picking ? t("permissions.picking") : t("permissions.grantFolder")}</button> : null}
        </div>
        <p className="mt-1">{t("permissions.grantsHint")}</p>
        <p className="mt-1">{t("permissions.externalHint")}</p>
        {loading ? <p role="status">{t("permissions.loading")}</p> : null}
        {!loading && !error && !grants?.length ? <p className="mt-2">{t("permissions.grantsEmpty")}</p> : null}
        <ul className="mt-2 divide-y divide-line">
          {grants?.map((grant) => <li key={grant.id} className="flex flex-wrap items-center justify-between gap-2 py-2">
            <span className="min-w-0 break-words">{grant.requirement.target}</span>
            <button type="button" className={button} disabled={revoking !== null} onClick={() => void revoke(grant.id)}>{revoking === grant.id ? t("permissions.revoking") : t("permissions.revoke")}</button>
          </li>)}
        </ul>
      </div>
      <div>
        <h2 className="font-medium">{t("permissions.instructions")}</h2>
        <p className="mt-1">{t("permissions.instructionsNote")}</p>
        {!loading && !error && !instructions?.sources.length ? <p className="mt-2">{t("permissions.instructionsEmpty")}</p> : null}
        <ul className="mt-2 divide-y divide-line">
          {instructions?.sources.map((source) => <li key={source.identityKey} className="py-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="min-w-0 break-all font-medium">{source.path}</span>
              <span className="text-[12px] text-secondary">{statusLabels[source.status]}</span>
            </div>
            <p className="mt-1 text-[12px] text-secondary">{t("permissions.version", { version: source.versionKey })}</p>
            {source.previewAvailable ? <pre className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-line bg-canvas p-2 text-[12px]">{source.content}</pre> : <p className="mt-1">{t("permissions.previewUnavailable")}</p>}
            {source.status === "pending" ? <div className="mt-2 flex flex-wrap gap-2">
              <button type="button" className={button} disabled={deciding !== null} onClick={() => void decide(source, true)}>{t("permissions.approve")}</button>
              <button type="button" className={button} disabled={deciding !== null} onClick={() => void decide(source, false)}>{t("permissions.deny")}</button>
              {deciding === source.identityKey ? <span role="status">{t("permissions.deciding")}</span> : null}
            </div> : null}
          </li>)}
        </ul>
      </div>
      {error || actionError ? <p role="alert">{t("permissions.error")} <button type="button" className={button} onClick={() => { setActionError(false); void refresh(); }}>{t("permissions.retry")}</button></p> : null}
  </div>;
}
