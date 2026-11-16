import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ChatParams } from "../lib/chat";
import { toFoundryCommandError } from "../lib/foundry";
import {
  paramsSetLayer,
  paramsSnapshot,
  type ParamScope,
  type ParamsSnapshot,
} from "../lib/params";
import { errorLabel } from "../workspaces/models/format";
import { useToast } from "./Toast";

type UseParamsLayerInput = {
  scope: ParamScope;
  conversationId?: number | null;
  modelAlias?: string | null;
  enabled?: boolean;
};

export function useParamsLayer({
  scope,
  conversationId = null,
  modelAlias = null,
  enabled = true,
}: UseParamsLayerInput) {
  const { t } = useTranslation();
  const { push } = useToast();
  const [snapshot, setSnapshot] = useState<ParamsSnapshot | null>(null);
  const [busy, setBusy] = useState(false);

  const apply = useCallback(
    async (params: ChatParams) => {
      if (busy) {
        return;
      }

      setBusy(true);

      try {
        const next = await paramsSetLayer({ scope, params, conversationId, modelAlias });

        setSnapshot(next);
        push(t("params.saved"), "neutral");
      } catch (error) {
        push(errorLabel(toFoundryCommandError(error), t), "danger");
      } finally {
        setBusy(false);
      }
    },
    [busy, conversationId, modelAlias, push, scope, t],
  );

  useEffect(() => {
    if (!enabled) {
      setSnapshot(null);

      return;
    }

    let live = true;

    paramsSnapshot(conversationId, modelAlias)
      .then((next) => {
        if (live) {
          setSnapshot(next);
        }
      })
      .catch((error) => {
        if (live) {
          push(errorLabel(toFoundryCommandError(error), t), "danger");
        }
      });

    return () => {
      live = false;
    };
  }, [conversationId, enabled, modelAlias, push, t]);

  return { snapshot, busy, apply };
}
