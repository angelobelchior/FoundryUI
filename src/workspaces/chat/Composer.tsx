import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CloseIcon, MicIcon, PaperclipIcon, SendIcon, StopIcon } from "../../components/Icon";
import { Modal } from "../../components/Modal";
import { Spinner } from "../../components/Spinner";
import { Tooltip } from "../../components/Tooltip";
import { useToast } from "../../components/Toast";
import { acceptedImageTypes, isAcceptedImage, type StagedImage } from "../../lib/attachments";
import { cx } from "../../lib/cx";
import {
  catalogList,
  loadedModels,
  toFoundryCommandError,
  unloadModel,
} from "../../lib/foundry";
import {
  cancelRecording,
  discardAudio,
  readSpeechModelPreference,
  recordingLevel,
  startRecording,
  stopRecording,
  transcribe,
  writeSpeechModelPreference,
} from "../../lib/speech";
import { errorLabel } from "../models/format";

type ComposerProps = {
  value: string;
  sending: boolean;
  active: boolean;
  stopping: boolean;
  modelLabel: string;
  attachments: StagedImage[];
  attachEnabled: boolean;
  attachHint: string;
  onChange: (value: string) => void;
  onSend: () => void;
  onStop: () => void;
  onAttach: (files: File[]) => void;
  onRemoveAttachment: (token: string) => void;
};

export function Composer({
  value,
  sending,
  active,
  stopping,
  modelLabel,
  attachments,
  attachEnabled,
  attachHint,
  onChange,
  onSend,
  onStop,
  onAttach,
  onRemoveAttachment,
}: ComposerProps) {
  const { t } = useTranslation();
  const { push } = useToast();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);
  const [recordingLevelValue, setRecordingLevelValue] = useState(0);
  const [transcribing, setTranscribing] = useState(false);
  const [pendingAudioPath, setPendingAudioPath] = useState<string | null>(null);
  const [undoDraft, setUndoDraft] = useState<{
    previous: string;
    selectionStart: number;
    selectionEnd: number;
  } | null>(null);
  const [liveMessage, setLiveMessage] = useState("");
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [memoryCandidates, setMemoryCandidates] = useState<string[]>([]);
  const [memoryBusyAlias, setMemoryBusyAlias] = useState<string | null>(null);

  useEffect(() => {
    const node = textareaRef.current;

    if (!node) {
      return;
    }

    node.style.height = "auto";
    node.style.height = `${Math.min(node.scrollHeight, 160)}px`;
  }, [value]);

  const canSend = !active && !sending && recordingStartedAt === null && !transcribing && (value.trim() !== "" || attachments.length > 0);
  const canAttach = attachEnabled && !active && !sending && recordingStartedAt === null && !transcribing;

  useEffect(() => {
    if (recordingStartedAt === null) {
      setRecordingLevelValue(0);

      return;
    }

    let live = true;
    const timer = window.setInterval(() => {
      recordingLevel()
        .then((level) => {
          if (live) {
            setRecordingLevelValue(level);
          }
        })
        .catch(() => {
          if (live) {
            setRecordingLevelValue(0);
          }
        });
    }, 80);

    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [recordingStartedAt]);

  useEffect(() => {
    return () => {
      if (pendingAudioPath) {
        void discardAudio(pendingAudioPath);
      }

      if (recordingStartedAt !== null) {
        void cancelRecording();
      }
    };
  }, [pendingAudioPath, recordingStartedAt]);

  async function resolveSpeechModelAlias(): Promise<string | null> {
    const preferred = readSpeechModelPreference();
    const catalog = await catalogList();
    const cachedSpeech = catalog.filter((model) => model.kind === "speech" && model.cached);

    if (!cachedSpeech.length) {
      push(t("chat.dictation.noSpeechModel"), "danger");

      return null;
    }

    const selected =
      preferred && cachedSpeech.some((model) => model.alias === preferred)
        ? preferred
        : cachedSpeech[0]?.alias ?? null;

    if (selected) {
      writeSpeechModelPreference(selected);
    }

    return selected;
  }

  function insertDictationText(text: string) {
    const trimmed = text.trim();

    if (!trimmed) {
      return;
    }

    const node = textareaRef.current;
    const selectionStart = node?.selectionStart ?? value.length;
    const selectionEnd = node?.selectionEnd ?? value.length;
    const next = `${value.slice(0, selectionStart)}${trimmed}${value.slice(selectionEnd)}`;
    const cursor = selectionStart + trimmed.length;

    setUndoDraft({
      previous: value,
      selectionStart,
      selectionEnd,
    });
    onChange(next);
    push(t("chat.dictation.inserted"), "neutral");
    window.requestAnimationFrame(() => {
      node?.focus();
      node?.setSelectionRange(cursor, cursor);
    });
  }

  async function transcribeAudio(path: string) {
    setTranscribing(true);
    setLiveMessage(t("chat.dictation.ariaTranscribing"));

    try {
      const modelAlias = await resolveSpeechModelAlias();

      if (!modelAlias) {
        setPendingAudioPath(null);
        await discardAudio(path);
        return;
      }

      const result = await transcribe({
        path,
        modelAlias,
        language: null,
        temperature: 0,
      });

      insertDictationText(result.transcription.text);

      if (result.autoLoaded) {
        push(t("chat.dictation.autoLoaded", { model: modelAlias }), "neutral");
      }

      setLiveMessage(t("chat.dictation.ariaDone"));
      setPendingAudioPath(null);
      await discardAudio(path);
    } catch (error) {
      const failure = toFoundryCommandError(error);

      if (failure.code === "OutOfMemory") {
        try {
          const summary = await loadedModels();
          const candidates = summary.models
            .map((model) => model.alias)
            .filter((alias) => alias !== modelLabel);

          if (candidates.length) {
            setMemoryCandidates(candidates);
            setMemoryOpen(true);
          } else {
            push(t("chat.dictation.noUnloadCandidates"), "danger");
            setPendingAudioPath(null);
            await discardAudio(path);
          }
        } catch (loadError) {
          push(errorLabel(toFoundryCommandError(loadError), t), "danger");
          setPendingAudioPath(null);
          await discardAudio(path);
        }
      } else {
        push(errorLabel(failure, t), "danger");
        setPendingAudioPath(null);
        await discardAudio(path);
      }

      setLiveMessage(t("chat.dictation.ariaFailed"));
    } finally {
      setTranscribing(false);
    }
  }

  function closeMemoryOffer() {
    setMemoryOpen(false);

    if (pendingAudioPath) {
      void discardAudio(pendingAudioPath);
      setPendingAudioPath(null);
    }
  }

  async function startDictation() {
    if (recordingStartedAt !== null || transcribing || active || sending) {
      return;
    }

    try {
      const startedAt = await startRecording();

      setRecordingStartedAt(startedAt);
      setLiveMessage(t("chat.dictation.ariaRecordingStarted"));
    } catch (error) {
      push(errorLabel(toFoundryCommandError(error), t), "danger");
    }
  }

  async function stopDictation() {
    if (recordingStartedAt === null || transcribing) {
      return;
    }

    try {
      const audio = await stopRecording();

      setRecordingStartedAt(null);
      setPendingAudioPath(audio.path);
      setLiveMessage(t("chat.dictation.ariaRecordingStopped"));
      await transcribeAudio(audio.path);
    } catch (error) {
      setRecordingStartedAt(null);
      push(errorLabel(toFoundryCommandError(error), t), "danger");
    }
  }

  async function cancelDictation() {
    if (recordingStartedAt === null) {
      return;
    }

    await cancelRecording();
    setRecordingStartedAt(null);
    setRecordingLevelValue(0);
    setLiveMessage(t("chat.dictation.ariaCancelled"));
  }

  async function unloadAndRetry(alias: string) {
    if (!pendingAudioPath) {
      return;
    }

    setMemoryBusyAlias(alias);

    try {
      await unloadModel(alias);
      setMemoryOpen(false);
      await transcribeAudio(pendingAudioPath);
    } catch (error) {
      push(errorLabel(toFoundryCommandError(error), t), "danger");
    } finally {
      setMemoryBusyAlias(null);
    }
  }

  function undoDictationInsertion() {
    if (!undoDraft) {
      return;
    }

    onChange(undoDraft.previous);

    const node = textareaRef.current;

    window.requestAnimationFrame(() => {
      node?.focus();
      node?.setSelectionRange(undoDraft.selectionStart, undoDraft.selectionEnd);
    });

    setUndoDraft(null);
  }

  function takeFiles(files: FileList | File[] | null) {
    const images = Array.from(files ?? []).filter((file) => file.type.startsWith("image/"));

    if (images.length && canAttach) {
      onAttach(images);
    }
  }

  return (
    <div className="shrink-0 border-t border-line bg-canvas px-3 py-2.5 sm:px-4 sm:py-3">
      <div className="mx-auto max-w-[760px]">
        <div
          onDragOver={(event) => {
            if (!canAttach) {
              return;
            }

            event.preventDefault();
            setDragging(true);
          }}
          onDragLeave={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
              setDragging(false);
            }
          }}
          onDrop={(event) => {
            event.preventDefault();
            setDragging(false);

            if (canAttach) {
              takeFiles(event.dataTransfer.files);
            }
          }}
          className={cx(
            "rounded-[10px] border bg-elevated transition-colors",
            dragging && canAttach
              ? "border-accent ring-2 ring-accent/20"
              : "border-line focus-within:border-line-strong",
          )}
        >
          {attachments.length ? (
            <div className="flex flex-wrap gap-2 px-2 pt-2">
              {attachments.map((attachment) => (
                <div
                  key={attachment.token}
                  className="group relative h-16 w-16 overflow-hidden rounded-lg border border-line bg-subtle"
                >
                  <img
                    src={attachment.previewUrl}
                    alt=""
                    className="h-full w-full object-cover"
                  />
                  <button
                    type="button"
                    onClick={() => onRemoveAttachment(attachment.token)}
                    aria-label={t("chat.removeAttachment")}
                    title={t("chat.removeAttachment")}
                    className="absolute right-0.5 top-0.5 inline-flex h-[18px] w-[18px] items-center justify-center rounded-full bg-scrim text-accent-contrast opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
                  >
                    <CloseIcon size={11} />
                  </button>
                </div>
              ))}
            </div>
          ) : null}
          <textarea
            ref={textareaRef}
            value={value}
            rows={1}
            disabled={active || sending || transcribing}
            placeholder={
              active || sending
                ? t("chat.streamingPlaceholder")
                : modelLabel
                  ? t("chat.composerPlaceholder", { model: modelLabel })
                  : t("chat.composerPlaceholderEmpty")
            }
            onChange={(event) => {
              setUndoDraft(null);
              onChange(event.target.value);
            }}
            onPaste={(event) => {
              const images = Array.from(event.clipboardData.files).filter(isAcceptedImage);

              if (!images.length || !canAttach) {
                return;
              }

              if (!event.clipboardData.getData("text/plain")) {
                event.preventDefault();
              }

              onAttach(images);
            }}
            onKeyDown={(event) => {
              if (
                (event.metaKey || event.ctrlKey) &&
                event.shiftKey &&
                event.key.toLowerCase() === "m"
              ) {
                event.preventDefault();

                if (recordingStartedAt === null) {
                  void startDictation();
                } else {
                  void stopDictation();
                }

                return;
              }

              if (event.key === "Escape" && recordingStartedAt !== null) {
                event.preventDefault();
                void cancelDictation();

                return;
              }

              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault();

                if (canSend) {
                  onSend();
                }
              }
            }}
            className="block max-h-40 w-full resize-none bg-transparent px-3 pt-2.5 text-[13px] leading-[1.5] text-primary outline-none placeholder:text-tertiary disabled:cursor-not-allowed"
          />
          <div className="flex flex-wrap items-center justify-between gap-2 px-2 pb-2 pt-1">
            <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
              <Tooltip label={attachHint} side="right">
                <button
                  type="button"
                  aria-label={t("chat.attach")}
                  disabled={!canAttach}
                  onClick={() => fileInputRef.current?.click()}
                  className={cx(
                    "inline-flex h-7 w-7 items-center justify-center rounded-md text-tertiary transition-colors",
                    "hover:bg-hover hover:text-primary",
                    "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-tertiary",
                  )}
                >
                  <PaperclipIcon size={14} />
                </button>
              </Tooltip>
              <Tooltip
                label={
                  transcribing
                    ? t("chat.dictation.transcribing")
                    : recordingStartedAt === null
                      ? t("chat.dictation.start")
                      : t("chat.dictation.stop")
                }
                side="right"
              >
                <button
                  type="button"
                  aria-label={
                    recordingStartedAt === null
                      ? t("chat.dictation.start")
                      : t("chat.dictation.stop")
                  }
                  disabled={active || sending || transcribing}
                  onClick={() =>
                    recordingStartedAt === null
                      ? void startDictation()
                      : void stopDictation()
                  }
                  className={cx(
                    "inline-flex h-7 w-7 items-center justify-center rounded-md text-tertiary transition-colors",
                    "hover:bg-hover hover:text-primary",
                    "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-tertiary",
                    recordingStartedAt !== null && "bg-active text-primary",
                  )}
                >
                  {transcribing ? <Spinner size={11} /> : recordingStartedAt === null ? <MicIcon size={14} /> : <StopIcon size={12} />}
                </button>
              </Tooltip>
              {undoDraft ? (
                <button
                  type="button"
                  onClick={undoDictationInsertion}
                  className="inline-flex h-7 items-center rounded-md px-2 text-[10.5px] text-tertiary transition-colors hover:bg-hover hover:text-primary"
                >
                  {t("chat.dictation.undo")}
                </button>
              ) : null}
              <span className="hidden truncate text-[10.5px] text-tertiary sm:inline">
                {recordingStartedAt !== null
                  ? t("chat.dictation.recording", { duration: formatDuration(Date.now() - recordingStartedAt) })
                  : active
                    ? t("chat.stopHint")
                    : t("chat.dictation.shortcut")}
              </span>
            </div>
            {active ? (
              <button
                type="button"
                onClick={onStop}
                disabled={stopping}
                className={cx(
                  "inline-flex h-7 items-center gap-1.5 rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium transition-colors",
                  "text-secondary hover:border-line-strong hover:text-primary",
                  "disabled:cursor-not-allowed disabled:opacity-60",
                )}
              >
                {stopping ? <Spinner size={12} /> : <StopIcon size={13} />}
                {stopping ? t("chat.stopping") : t("chat.stop")}
              </button>
            ) : (
              <Tooltip label={t("chat.send")} side="right">
                <button
                  type="button"
                  onClick={onSend}
                  disabled={!canSend || transcribing}
                  aria-label={t("chat.send")}
                  title={t("chat.send")}
                  className={cx(
                    "inline-flex h-8 w-8 items-center justify-center rounded-[8px] bg-accent text-accent-contrast transition-opacity",
                    "hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-40",
                  )}
                >
                  <SendIcon size={14} />
                </button>
              </Tooltip>
            )}
          </div>
          {recordingStartedAt !== null ? (
            <div className="px-2 pb-2">
              <div className="h-1.5 overflow-hidden rounded-full bg-subtle">
                <div
                  className="h-full rounded-full bg-accent transition-[width] duration-75"
                  style={{ width: `${Math.max(4, Math.round(recordingLevelValue * 100))}%` }}
                />
              </div>
              <div className="mt-1 flex justify-end">
                <button
                  type="button"
                  onClick={() => void cancelDictation()}
                  className="inline-flex h-6 items-center rounded-md px-2 text-[10.5px] text-tertiary transition-colors hover:bg-hover hover:text-primary"
                >
                  {t("chat.dictation.cancel")}
                </button>
              </div>
            </div>
          ) : null}
          <p className="sr-only" role="status" aria-live="polite">
            {liveMessage}
          </p>
        </div>
        <input
          ref={fileInputRef}
          type="file"
          accept={acceptedImageTypes.join(",")}
          multiple
          hidden
          onChange={(event) => {
            takeFiles(event.target.files);
            event.target.value = "";
          }}
        />
      </div>

      <Modal
        open={memoryOpen}
        title={t("chat.dictation.memoryTitle")}
        onClose={closeMemoryOffer}
      >
        <div className="space-y-3">
          <p className="text-[12px] leading-relaxed text-secondary">{t("chat.dictation.memoryBody")}</p>
          <div className="space-y-2">
            {memoryCandidates.map((alias) => (
              <button
                key={alias}
                type="button"
                onClick={() => void unloadAndRetry(alias)}
                disabled={memoryBusyAlias !== null}
                className="flex h-8 w-full items-center justify-between rounded-[7px] border border-line px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60"
              >
                <span>{alias}</span>
                {memoryBusyAlias === alias ? <Spinner size={11} /> : null}
              </button>
            ))}
          </div>
          <div className="flex justify-end">
            <button
              type="button"
              onClick={closeMemoryOffer}
              className="inline-flex h-7 items-center rounded-[7px] border border-line px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary"
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

function formatDuration(durationMs: number): string {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;

  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
