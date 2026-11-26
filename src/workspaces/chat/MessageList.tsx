import { Suspense, lazy, memo, useEffect, useRef, useState, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { Caret } from "../../components/Caret";
import { CopyIcon, PencilIcon, RefreshIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { attachmentData } from "../../lib/attachments";
import { copyText } from "../../lib/copy";
import { cx } from "../../lib/cx";
import { isFoundryErrorCode } from "../../lib/foundry";
import type { ChatAttachment, ChatMessage } from "../../lib/chat";
import { paramFields, type ResolvedParams } from "../../lib/params";
import type { AgentToolCard } from "../../lib/agent";
import { AgentActivity } from "./ToolCard";

type MessageListProps = {
  messages: ChatMessage[];
  hasMore: boolean;
  loadingOlder: boolean;
  highlightId: number | null;
  actionsDisabled: boolean;
  onLoadOlder: () => void;
  onEdit: (message: ChatMessage) => void;
  onRegenerate: () => void;
  toolsByMessage?: Record<number, AgentToolCard[]>;
};

const Markdown = lazy(() =>
  import("../../components/markdown/Markdown").then((module) => ({ default: module.Markdown })),
);

export function MessageList({
  messages,
  hasMore,
  loadingOlder,
  highlightId,
  actionsDisabled,
  onLoadOlder,
  onEdit,
  onRegenerate,
  toolsByMessage,
}: MessageListProps) {
  const { t, i18n } = useTranslation();
  const scrollerRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const scrolledFor = useRef<number | null>(null);
  const lastMessage = [...messages].reverse().find((message) => message.role !== "divider");
  const regenerateId = lastMessage?.role === "assistant" ? lastMessage.id : null;

  useEffect(() => {
    const node = scrollerRef.current;

    if (node && stickToBottom.current) {
      node.scrollTop = node.scrollHeight;
    }
  }, [messages]);

  useEffect(() => {
    if (highlightId === null) {
      scrolledFor.current = null;

      return;
    }

    if (scrolledFor.current === highlightId) {
      return;
    }

    const node = scrollerRef.current?.querySelector<HTMLElement>(
      `[data-message-id="${highlightId}"]`,
    );

    if (!node) {
      return;
    }

    scrolledFor.current = highlightId;
    stickToBottom.current = false;
    node.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [highlightId, messages]);

  return (
    <div
      ref={scrollerRef}
      onScroll={(event) => {
        const node = event.currentTarget;
        stickToBottom.current = node.scrollHeight - node.scrollTop - node.clientHeight < 96;
      }}
      className="min-h-0 flex-1 overflow-y-auto px-3 py-4 sm:px-5 sm:py-5"
    >
      <div className="mx-auto flex max-w-[780px] flex-col gap-3.5 sm:gap-4">
        {hasMore ? (
          <button
            type="button"
            onClick={onLoadOlder}
            disabled={loadingOlder}
            className="mx-auto inline-flex h-7 items-center gap-2 rounded-full border border-line px-3 text-[11px] text-secondary transition-colors hover:border-line-strong hover:text-primary disabled:opacity-60"
          >
            {loadingOlder ? <Spinner size={12} /> : null}
            {t("chat.loadOlder")}
          </button>
        ) : null}
        {messages.length === 0 ? (
          <div className="flex flex-col items-center px-6 py-16 text-center">
            <p className="text-[13px] font-medium text-primary">{t("chat.conversationEmptyTitle")}</p>
            <p className="mt-1 max-w-[36ch] text-[12px] leading-relaxed text-secondary">
              {t("chat.conversationEmptyBody")}
            </p>
          </div>
        ) : null}
        {messages.map((message) => (
          <div
            key={message.id}
            data-message-id={message.id}
            className={cx(
              "-mx-1.5 rounded-xl px-1.5 transition-colors duration-500 sm:-mx-2 sm:px-2",
              highlightId === message.id && "bg-accent/10 ring-1 ring-accent/25 ring-inset",
            )}
          >
            {message.role === "divider" ? (
              <Divider model={message.modelAlias ?? ""} />
            ) : (
              <MessageBubble
                message={message}
                locale={i18n.language}
                tools={toolsByMessage?.[message.id] ?? []}
                onEdit={
                  message.role === "user" && !actionsDisabled
                    ? () => onEdit(message)
                    : undefined
                }
                onRegenerate={
                  message.id === regenerateId && !actionsDisabled ? onRegenerate : undefined
                }
              />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function Divider({ model }: { model: string }) {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-3 py-1" role="separator">
      <span aria-hidden="true" className="h-px flex-1 bg-line" />
      <span className="shrink-0 text-[10.5px] text-tertiary">
        {t("chat.modelChanged", { model })}
      </span>
      <span aria-hidden="true" className="h-px flex-1 bg-line" />
    </div>
  );
}

type MessageBubbleProps = {
  message: ChatMessage;
  locale: string;
  tools: AgentToolCard[];
  onEdit?: (() => void) | undefined;
  onRegenerate?: (() => void) | undefined;
};

const MessageBubble = memo(function MessageBubble({
  message,
  locale,
  tools,
  onEdit,
  onRegenerate,
}: MessageBubbleProps) {
  const { t } = useTranslation();
  const { push } = useToast();
  const isUser = message.role === "user";
  const streaming = message.status === "streaming";
  const waitingFirstToken = streaming && message.content === "" && message.reasoning === "";
  const numberFormat = new Intl.NumberFormat(locale);
  const errorCode = message.error?.code;
  const errorText =
    errorCode && isFoundryErrorCode(errorCode)
      ? t(`errors.${errorCode}`)
      : t("chat.status.error");

  async function copyMessage() {
    const copied = await copyText(message.content);

    push(copied ? t("chat.copied") : t("chat.copyFailed"), copied ? "neutral" : "danger");
  }

  return (
    <div className={cx("flex", isUser ? "justify-end" : "justify-start")}>
      <div
        className={cx(
          "min-w-0",
          isUser
            ? "max-w-[86%] rounded-2xl bg-accent px-3.5 py-2 text-accent-contrast sm:max-w-[78%]"
            : "w-full max-w-[720px] text-primary",
        )}
      >
        {message.attachments.length ? (
          <MessageAttachments attachments={message.attachments} align={isUser ? "end" : "start"} />
        ) : null}
        {message.reasoning ? (
          <div className="mb-2 rounded-lg border border-line bg-subtle px-3 py-2">
            <div className="mb-1 text-[10px] font-semibold uppercase tracking-[0.055em] text-tertiary">
              {t("chat.reasoning")}
            </div>
            <p className="whitespace-pre-wrap break-words text-[12px] leading-[1.55] text-secondary">
              {message.reasoning}
            </p>
          </div>
        ) : null}
        {!isUser ? <AgentActivity tools={tools} /> : null}
        {message.content ? (
          isUser ? (
            <p className="whitespace-pre-wrap break-words text-[13px] leading-[1.6]">
              {message.content}
            </p>
          ) : (
            <Suspense
              fallback={
                <p className="whitespace-pre-wrap break-words text-[13px] leading-[1.6]">
                  {message.content}
                </p>
              }
            >
              <Markdown content={message.content} streaming={streaming} />
            </Suspense>
          )
        ) : waitingFirstToken ? (
          <div role="status" className="flex items-center gap-2 text-[12px] text-secondary">
            <Spinner size={12} className="text-tertiary" />
            <span>{t("chat.processingReply")}</span>
          </div>
        ) : streaming ? (
          <Caret />
        ) : null}
        {message.status === "cancelled" ? (
          <p className={cx("mt-1.5 text-[10.5px]", isUser ? "text-accent-contrast/75" : "text-tertiary")}>
            {t("chat.status.cancelled")}
          </p>
        ) : null}
        {message.status === "interrupted" ? (
          <p className="mt-1.5 text-[10.5px] text-tertiary">{t("chat.status.interrupted")}</p>
        ) : null}
        {message.status === "error" ? (
          <p className="mt-1.5 text-[11px] text-danger">{errorText}</p>
        ) : null}
        <div className={cx("mt-1.5 flex items-center gap-2", isUser ? "justify-end" : null)}>
          <time
            dateTime={new Date(message.createdAt).toISOString()}
            title={formatMessageDate(message.createdAt, locale)}
            className={cx(
              "text-[10.5px] tabular-nums",
              isUser ? "text-accent-contrast/70" : "text-tertiary",
            )}
          >
            {formatMessageTime(message.createdAt, locale)}
          </time>
          {message.usage && !isUser ? (
            <p className="text-[10.5px] tabular-nums text-tertiary">
              {t("chat.usage", {
                prompt: numberFormat.format(message.usage.promptTokens),
                completion: numberFormat.format(message.usage.completionTokens),
              })}
            </p>
          ) : null}
          {message.content ? (
            <MessageAction label={t("chat.copyMessage")} isUser={isUser} onClick={() => void copyMessage()}>
              <CopyIcon size={12} />
            </MessageAction>
          ) : null}
          {onEdit ? (
            <MessageAction label={t("chat.edit.label")} isUser={isUser} onClick={onEdit}>
              <PencilIcon size={12} />
            </MessageAction>
          ) : null}
          {onRegenerate ? (
            <MessageAction label={t("chat.regenerate.label")} isUser={isUser} onClick={onRegenerate}>
              <RefreshIcon size={12} />
            </MessageAction>
          ) : null}
        </div>
        {message.params ? <MessageParams params={message.params} /> : null}
      </div>
    </div>
  );
});

function MessageAction({
  label,
  isUser,
  onClick,
  children,
}: {
  label: string;
  isUser: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
        className={cx(
          "inline-flex h-6 items-center rounded px-1.5 transition-colors",
          isUser
            ? "text-accent-contrast/65 hover:bg-white/15 hover:text-accent-contrast"
            : "text-tertiary hover:bg-hover hover:text-secondary",
      )}
    >
      {children}
    </button>
  );
}

function MessageAttachments({
  attachments,
  align,
}: {
  attachments: ChatAttachment[];
  align: "start" | "end";
}) {
  return (
    <div className={cx("mb-2 flex flex-wrap gap-2", align === "end" ? "justify-end" : "justify-start")}>
      {attachments.map((attachment) => (
        <AttachmentImage key={attachment.id} attachment={attachment} />
      ))}
    </div>
  );
}

function AttachmentImage({ attachment }: { attachment: ChatAttachment }) {
  const [source, setSource] = useState<string | null>(null);

  useEffect(() => {
    let live = true;

    attachmentData(attachment.id)
      .then((url) => {
        if (live) {
          setSource(url);
        }
      })
      .catch(() => undefined);

    return () => {
      live = false;
    };
  }, [attachment.id]);

  if (!source) {
    return <div className="h-24 w-24 animate-pulse rounded-lg bg-subtle" aria-hidden="true" />;
  }

  return (
    <img
      src={source}
      alt={attachment.mime}
      className="max-h-44 max-w-[240px] rounded-lg border border-line object-contain"
    />
  );
}

function MessageParams({ params }: { params: ResolvedParams }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const entries = paramFields.flatMap((field) => {
    const resolved = params[field];

    if (resolved.value === null) {
      return [];
    }

    return [
      {
        field,
        origin: resolved.origin,
        value: formatParamValue(field, resolved.value, t),
      },
    ];
  });

  if (!entries.length) {
    return null;
  }

  return (
    <div className="mt-1">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        className="text-[10.5px] text-tertiary transition-colors hover:text-secondary"
      >
        {open ? t("chat.hideDetails") : t("chat.details")}
      </button>
      {open ? (
        <dl className="mt-1 flex flex-wrap gap-x-3 gap-y-1">
          {entries.map((entry) => (
            <div key={entry.field} className="flex items-baseline gap-1 text-[10.5px]">
              <dt className="text-tertiary">{t(`params.fields.${entry.field}`)}</dt>
              <dd className="tabular-nums text-secondary">
                {entry.value}
                <span className="text-tertiary"> · {t(`params.origin.${entry.origin}`)}</span>
              </dd>
            </div>
          ))}
        </dl>
      ) : null}
    </div>
  );
}

function formatParamValue(field: (typeof paramFields)[number], value: unknown, t: TFunction) {
  if (Array.isArray(value)) {
    return value.join(", ");
  }

  if (field === "reasoningEffort" && typeof value === "string") {
    return t(`params.options.reasoningEffort.${value}` as never);
  }

  if (field === "responseVerbosity" && typeof value === "string") {
    return t(`params.options.responseVerbosity.${value}` as never);
  }

  return String(value);
}

function formatMessageTime(timestamp: number, locale: string): string {
  const date = new Date(timestamp);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();

  const options: Intl.DateTimeFormatOptions = sameDay
    ? { hour: "2-digit", minute: "2-digit" }
    : { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" };

  return new Intl.DateTimeFormat(locale, options).format(date);
}

function formatMessageDate(timestamp: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}
