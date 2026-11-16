import type { ParamScope } from "../lib/params";
import { ParamsEditor } from "./ParamsEditor";
import { Spinner } from "./Spinner";
import { useParamsLayer } from "./useParamsLayer";

type ParamsSectionProps = {
  scope: ParamScope;
  help: string;
  conversationId?: number | null;
  modelAlias?: string | null;
};

export function ParamsSection({
  scope,
  help,
  conversationId = null,
  modelAlias = null,
}: ParamsSectionProps) {
  const { snapshot, busy, apply } = useParamsLayer({ scope, conversationId, modelAlias });

  return (
    <div>
      <p className="mb-3 max-w-[56ch] text-[11.5px] leading-relaxed text-tertiary">{help}</p>
      {snapshot ? (
        <ParamsEditor
          snapshot={snapshot}
          scope={scope}
          busy={busy}
          onSave={(params) => void apply(params)}
        />
      ) : (
        <div className="flex justify-center py-5">
          <Spinner size={13} className="text-tertiary" />
        </div>
      )}
    </div>
  );
}
