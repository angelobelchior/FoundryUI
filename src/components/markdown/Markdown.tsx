import {
  Children,
  isValidElement,
  memo,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeSlug from "rehype-slug";
import { openUrl } from "@tauri-apps/plugin-opener";
import "katex/dist/katex.min.css";
import { Caret } from "../Caret";
import { splitMarkdownBlocks } from "../../lib/markdown";
import { CodeBlock } from "./CodeBlock";
import { MermaidBlock } from "./MermaidBlock";

const katexOptions = {
  throwOnError: false,
  strict: "ignore" as const,
  trust: false,
  maxSize: 16,
  maxExpand: 1000,
  errorColor: "var(--danger)",
};

const components: Components = {
  pre({ children }) {
    const child = Children.toArray(children)[0];

    if (!isValidElement<{ className?: string; children?: ReactNode }>(child)) {
      return <pre>{children}</pre>;
    }

    const language = /language-([\w+#.-]+)/.exec(child.props.className ?? "")?.[1]?.toLowerCase();
    const code = String(child.props.children ?? "").replace(/\n$/, "");

    if (language === "mermaid") {
      return <MermaidBlock code={code} />;
    }

    return <CodeBlock code={code} language={language ?? null} />;
  },
  code({ children }) {
    return <code className="md-inline-code">{children}</code>;
  },
  a({ href, children }) {
    return <MarkdownLink href={href}>{children}</MarkdownLink>;
  },
  img({ src, alt }) {
    if (!src) {
      return null;
    }

    return <img src={src} alt={alt ?? ""} loading="lazy" decoding="async" className="md-image" />;
  },
  table({ children }) {
    return (
      <div className="md-table">
        <table>{children}</table>
      </div>
    );
  },
};

type MarkdownProps = {
  content: string;
  streaming: boolean;
};

export function Markdown({ content, streaming }: MarkdownProps) {
  if (!streaming) {
    return (
      <div className="markdown">
        <MarkdownDocument content={content} />
      </div>
    );
  }

  const blocks = splitMarkdownBlocks(content, true);
  const unfinished = blocks.at(-1)?.open ?? true;

  return (
    <div className="markdown">
      {blocks.map((block, index) =>
        block.open ? (
          <StreamingTail key={index} content={block.content} />
        ) : (
          <MarkdownBlock key={index} content={block.content} />
        ),
      )}
      {unfinished ? null : <Caret />}
    </div>
  );
}

const MarkdownBlock = memo(function MarkdownBlock({ content }: { content: string }) {
  return <MarkdownDocument content={content} />;
});

function MarkdownDocument({ content }: { content: string }) {
  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={[rehypeSlug, [rehypeKatex, katexOptions]]}
      components={components}
    >
      {content}
    </ReactMarkdown>
  );
}

function StreamingTail({ content }: { content: string }) {
  return (
    <div className="md-tail">
      {content}
      <Caret />
    </div>
  );
}

function MarkdownLink({ href, children }: { href: string | undefined; children: ReactNode }) {
  function follow(event: ReactMouseEvent<HTMLAnchorElement>) {
    if (!href) {
      return;
    }

    if (href.startsWith("#")) {
      event.preventDefault();

      const container = event.currentTarget.closest(".markdown");
      const id = decodeURIComponent(href.slice(1));
      const target = container
        ? Array.from(container.querySelectorAll<HTMLElement>("[id]")).find((node) => node.id === id)
        : document.getElementById(id);

      target?.scrollIntoView({ behavior: "smooth", block: "center" });
      return;
    }

    event.preventDefault();

    if (/^(https?:|mailto:)/i.test(href)) {
      void openUrl(href).catch(() => window.open(href, "_blank", "noreferrer"));
    }
  }

  return (
    <a href={href} onClick={follow} rel="noreferrer" className="md-link">
      {children}
    </a>
  );
}
