import { useRef, useState } from "react";
import { projectApi } from "../../../lib/projects";

export type EditorTab = {
  id: string;
  projectId: number;
  path: string;
  name: string;
  content: string;
  savedContent: string;
  status: "loading" | "ready" | "error";
};

const AUTO_SAVE_DELAY = 800;

export function useEditorTabs() {
  const [tabs, setTabs] = useState<EditorTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const tabsRef = useRef<EditorTab[]>([]);
  const seqRef = useRef(0);
  const saveTimersRef = useRef<Map<string, number>>(new Map());
  tabsRef.current = tabs;

  async function openFile(projectId: number, path: string) {
    const id = `${projectId}\u0000${path}`;

    if (tabsRef.current.some((tab) => tab.id === id)) {
      setActiveId(id);
      return;
    }

    const name = path.split("/").pop() ?? path;
    setTabs((current) => [
      ...current,
      { id, projectId, path, name, content: "", savedContent: "", status: "loading" },
    ]);
    setActiveId(id);

    const version = ++seqRef.current;
    try {
      const file = await projectApi.readFile(projectId, path);
      if (version !== seqRef.current) return;
      setTabs((current) =>
        current.map((tab) =>
          tab.id === id
            ? { ...tab, content: file.content, savedContent: file.content, status: "ready" }
            : tab,
        ),
      );
    } catch {
      if (version !== seqRef.current) return;
      setTabs((current) =>
        current.map((tab) => (tab.id === id ? { ...tab, status: "error" } : tab)),
      );
    }
  }

  function closeFile(id: string) {
    const timer = saveTimersRef.current.get(id);
    if (timer) window.clearTimeout(timer);
    saveTimersRef.current.delete(id);
    const closing = tabsRef.current.find((tab) => tab.id === id);
    if (closing && closing.content !== closing.savedContent) {
      void save(id);
    }
    setTabs((current) => {
      const index = current.findIndex((tab) => tab.id === id);
      const next = current.filter((tab) => tab.id !== id);
      if (activeId === id) {
        const neighbor = next[Math.min(Math.max(index, 0), next.length - 1)];
        setActiveId(neighbor?.id ?? null);
      }
      return next;
    });
  }

  function closeAll() {
    for (const timer of saveTimersRef.current.values()) window.clearTimeout(timer);
    saveTimersRef.current.clear();
    for (const tab of tabsRef.current) {
      if (tab.content !== tab.savedContent) {
        void save(tab.id);
      }
    }
    setTabs([]);
    setActiveId(null);
  }

  function updateContent(id: string, content: string) {
    setTabs((current) =>
      current.map((tab) => (tab.id === id ? { ...tab, content } : tab)),
    );
    const existing = saveTimersRef.current.get(id);
    if (existing) window.clearTimeout(existing);
    const timer = window.setTimeout(() => {
      saveTimersRef.current.delete(id);
      void save(id);
    }, AUTO_SAVE_DELAY);
    saveTimersRef.current.set(id, timer);
  }

  async function save(id: string) {
    const tab = tabsRef.current.find((item) => item.id === id);
    if (!tab) return;
    const content = tab.content;
    await projectApi.writeFile(tab.projectId, tab.path, content);
    setTabs((current) =>
      current.map((item) =>
        item.id === id && item.content === content ? { ...item, savedContent: content } : item,
      ),
    );
  }

  const active = tabs.find((tab) => tab.id === activeId) ?? null;

  function isDirty(id: string) {
    const tab = tabs.find((item) => item.id === id);
    return tab ? tab.content !== tab.savedContent : false;
  }

  return {
    tabs,
    active,
    activeId,
    openFile,
    closeFile,
    closeAll,
    setActiveId,
    updateContent,
    save,
    isDirty,
  };
}
