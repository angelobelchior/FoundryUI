import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import DOMPurify from "dompurify";
import mermaidScriptUrl from "mermaid/dist/mermaid.min.js?url";
import { Spinner } from "../Spinner";
import { useResolvedThemeMode } from "../../lib/theme";
import { CodeBlock } from "./CodeBlock";

type MermaidState =
  | { status: "loading" }
  | { status: "ready"; svg: string }
  | { status: "error" };

type MermaidRuntime = typeof import("mermaid").default;

let mermaidPromise: Promise<MermaidRuntime> | null = null;

function readMermaidGlobal() {
  return (window as Window & { mermaid?: MermaidRuntime }).mermaid ?? null;
}

function loadMermaidScript() {
  return new Promise<void>((resolve, reject) => {
    const existing = readMermaidGlobal();

    if (existing) {
      resolve();
      return;
    }

    const id = "foundryui-mermaid-runtime";
    const current = document.getElementById(id) as HTMLScriptElement | null;

    const onLoad = () => {
      resolve();
    };

    const onError = () => {
      reject(new Error("mermaid runtime failed to load"));
    };

    if (current) {
      current.addEventListener("load", onLoad, { once: true });
      current.addEventListener("error", onError, { once: true });
      return;
    }

    const script = document.createElement("script");

    script.id = id;
    script.async = true;
    script.src = mermaidScriptUrl;
    script.addEventListener("load", onLoad, { once: true });
    script.addEventListener("error", onError, { once: true });
    document.head.append(script);
  });
}

function loadMermaid() {
  mermaidPromise ??= (async () => {
    const existing = readMermaidGlobal();

    if (existing) {
      return existing;
    }

    await loadMermaidScript();

    const loaded = readMermaidGlobal();

    if (!loaded) {
      throw new Error("mermaid runtime unavailable");
    }

    return loaded;
  })().catch((error) => {
    mermaidPromise = null;
    throw error;
  });

  return mermaidPromise;
}

export function MermaidBlock({ code }: { code: string }) {
  const { t } = useTranslation();
  const mode = useResolvedThemeMode();
  const baseId = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const counter = useRef(0);
  const [state, setState] = useState<MermaidState>({ status: "loading" });

  useEffect(() => {
    let live = true;

    setState({ status: "loading" });

    void (async () => {
      try {
        const mermaid = await loadMermaid();

        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: mode === "dark" ? "dark" : "default",
          fontFamily: '-apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif',
          suppressErrorRendering: true,
          htmlLabels: false,
        });

        counter.current += 1;

        const { svg } = await mermaid.render(`mermaid-${baseId}-${counter.current}`, code);
        const clean = String(
          DOMPurify.sanitize(svg, {
            USE_PROFILES: { svg: true, svgFilters: true, html: true },
            ADD_TAGS: ["foreignObject"],
            FORBID_TAGS: ["script", "iframe", "object", "embed", "form"],
          }),
        );

        if (live) {
          setState({ status: "ready", svg: clean });
        }
      } catch {
        if (live) {
          setState({ status: "error" });
        }
      }
    })();

    return () => {
      live = false;
    };
  }, [baseId, code, mode]);

  if (state.status === "error") {
    return (
      <div className="my-3">
        <p className="mb-1.5 text-[11px] text-danger">{t("markdown.mermaidError")}</p>
        <CodeBlock code={code} language="mermaid" />
      </div>
    );
  }

  if (state.status === "loading") {
    return (
      <div className="md-mermaid flex items-center gap-2 text-[11px] text-tertiary">
        <Spinner size={12} />
        {t("markdown.mermaidRendering")}
      </div>
    );
  }

  return <div className="md-mermaid" dangerouslySetInnerHTML={{ __html: state.svg }} />;
}
