import { invoke } from "@tauri-apps/api/core";
import { Copy, ExternalLink, Info, Pin, Scissors, Trash2, X } from "lucide-react";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import ReactDOM from "react-dom/client";
import "./popup.css";

const POPUP_PAGE_SIZE = 10;

type ClipboardEntry = {
  id: string;
  text: string;
  title?: string | null;
  source: string;
  pinnedAt?: number | null;
  deletedAt?: number | null;
  createdAt: number;
  lastCopiedAt: number;
  lastUsedAt?: number | null;
  copyCount: number;
  useCount: number;
};

type ClipboardQueryResult = {
  entries: ClipboardEntry[];
  total: number;
};

function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => reject(new Error(`${label} timeout`)), ms);
    promise.then(
      (value) => {
        window.clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        window.clearTimeout(timer);
        reject(error);
      },
    );
  });
}

function PopupApp() {
  const [allEntries, setAllEntries] = useState<ClipboardEntry[]>([]);
  const [search, setSearch] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [page, setPage] = useState(0);
  const [pageAnimating, setPageAnimating] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [toastClosing, setToastClosing] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [openActionId, setOpenActionId] = useState<string | null>(null);
  const [detailEntry, setDetailEntry] = useState<ClipboardEntry | null>(null);
  const [extractEntry, setExtractEntry] = useState<ClipboardEntry | null>(null);
  const [closingDialog, setClosingDialog] = useState<"detail" | "extract" | null>(null);
  const [selectedTokens, setSelectedTokens] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLElement>(null);
  const entryRefs = useRef(new Map<string, HTMLElement>());
  const shouldRevealSelectedRef = useRef(false);
  const toastTimerRef = useRef<number | null>(null);
  const toastCloseTimerRef = useRef<number | null>(null);

  const pageCount = Math.max(1, Math.ceil(allEntries.length / POPUP_PAGE_SIZE));
  const pageEntries = useMemo(
    () => allEntries.slice(page * POPUP_PAGE_SIZE, page * POPUP_PAGE_SIZE + POPUP_PAGE_SIZE),
    [allEntries, page],
  );
  const selectedEntry = pageEntries[selectedIndex];
  const extractTokens = useMemo(() => extractClipboardTokens(extractEntry?.text ?? ""), [extractEntry]);

  const showToast = useCallback((message: string) => {
    if (toastTimerRef.current) {
      window.clearTimeout(toastTimerRef.current);
    }
    if (toastCloseTimerRef.current) {
      window.clearTimeout(toastCloseTimerRef.current);
    }
    setToastClosing(false);
    setToast(message);
    toastTimerRef.current = window.setTimeout(() => {
      setToastClosing(true);
      toastCloseTimerRef.current = window.setTimeout(() => {
        setToast(null);
        setToastClosing(false);
      }, 180);
    }, 980);
  }, []);

  const playPageAnimation = useCallback(() => {
    setPageAnimating(false);
    window.requestAnimationFrame(() => {
      setPageAnimating(true);
      window.setTimeout(() => setPageAnimating(false), 180);
    });
  }, []);

  const loadEntries = useCallback(async (showBusy = true) => {
    if (showBusy) {
      setLoading(true);
    }
    try {
      const [pinnedResult, historyResult] = await withTimeout(
        Promise.all([
          invoke<ClipboardQueryResult>("clipboard_query", { input: { scope: "pinned", search, offset: 0, limit: 500 } }),
          invoke<ClipboardQueryResult>("clipboard_query", { input: { scope: "history", search, offset: 0, limit: 500 } }),
        ]),
        1800,
        "clipboard query",
      );
      setAllEntries([...pinnedResult.entries, ...historyResult.entries]);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (showBusy) {
        setLoading(false);
      }
    }
  }, [search]);

  useEffect(() => {
    void invoke("push_frontend_debug_log", { level: "info", message: "clipboard popup rewrite: mounted" });
    inputRef.current?.focus();
    return () => {
      if (toastTimerRef.current) {
        window.clearTimeout(toastTimerRef.current);
      }
      if (toastCloseTimerRef.current) {
        window.clearTimeout(toastCloseTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    setPage(0);
    setSelectedIndex(0);
    setOpenActionId(null);
  }, [search]);

  useEffect(() => {
    const timer = window.setTimeout(() => void loadEntries(), 120);
    return () => window.clearTimeout(timer);
  }, [loadEntries]);

  useEffect(() => {
    function handleDocumentKeyDown(event: KeyboardEvent) {
      handleNavigationKey(event.key, event);
    }
    window.addEventListener("keydown", handleDocumentKeyDown, true);
    return () => window.removeEventListener("keydown", handleDocumentKeyDown, true);
  });

  useEffect(() => {
    const timer = window.setInterval(() => void loadEntries(false), 900);
    return () => window.clearInterval(timer);
  }, [loadEntries]);

  useEffect(() => {
    if (page > 0 && page >= pageCount) {
      setPage(pageCount - 1);
      setSelectedIndex(0);
    }
  }, [page, pageCount]);

  useEffect(() => {
    setSelectedIndex((index) => Math.min(index, Math.max(0, pageEntries.length - 1)));
  }, [pageEntries.length]);

  useEffect(() => {
    if (!shouldRevealSelectedRef.current || !selectedEntry || detailEntry || extractEntry) {
      return;
    }
    shouldRevealSelectedRef.current = false;
    if (selectedIndex === 0) {
      listRef.current?.scrollTo({ top: 0 });
      return;
    }
    entryRefs.current.get(selectedEntry.id)?.scrollIntoView({ block: "nearest" });
  }, [selectedEntry, selectedIndex, detailEntry, extractEntry]);

  async function closePopup() {
    await invoke("clipboard_close_panel");
  }

  function toggleWindowPinned() {
    setPinned((current) => {
      const next = !current;
      void invoke("clipboard_set_panel_pinned", { pinned: next });
      return next;
    });
  }

  function setWindowDragging(dragging: boolean) {
    void invoke("clipboard_set_panel_dragging", { dragging });
    if (dragging) {
      window.setTimeout(() => void invoke("clipboard_set_panel_dragging", { dragging: false }), 900);
    }
  }

  function startWindowDrag() {
    void invoke("clipboard_start_panel_drag");
  }

  function changePage(nextPage: number) {
    const safePage = Math.max(0, Math.min(nextPage, pageCount - 1));
    if (safePage === page) {
      return;
    }
    playPageAnimation();
    setPage(safePage);
    setSelectedIndex(0);
    setOpenActionId(null);
    window.requestAnimationFrame(() => listRef.current?.scrollTo({ top: 0 }));
  }

  async function inputEntryAndClose(entry: ClipboardEntry) {
    try {
      await withTimeout(invoke<{ message: string }>("clipboard_paste", { id: entry.id }), 1400, "clipboard paste");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function copyEntry(entry: ClipboardEntry) {
    try {
      const result = await withTimeout(invoke<{ message: string }>("clipboard_copy", { id: entry.id }), 1200, "clipboard copy");
      showToast(result.message || "已复制");
      await loadEntries(false);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function togglePinned(entry: ClipboardEntry) {
    try {
      await withTimeout(
        invoke("clipboard_update_entry", { id: entry.id, patch: { pinned: !entry.pinnedAt } }),
        1200,
        "clipboard pin",
      );
      showToast(entry.pinnedAt ? "已取消固定" : "已固定");
      await loadEntries();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function deleteEntry(entry: ClipboardEntry) {
    try {
      await withTimeout(invoke("clipboard_delete", { ids: [entry.id] }), 1200, "clipboard delete");
      setOpenActionId(null);
      showToast("已移入回收站");
      await loadEntries();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function copyExtractedTokens() {
    if (selectedTokens.length === 0) {
      return;
    }
    try {
      const result = await withTimeout(
        invoke<{ message: string }>("clipboard_copy_derived_text", { text: selectedTokens.join("") }),
        1200,
        "clipboard token copy",
      );
      setExtractEntry(null);
      setSelectedTokens([]);
      showToast(result.message || "已复制提取内容");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  async function pasteTokenAndClose(token: string) {
    try {
      await withTimeout(invoke<{ message: string }>("clipboard_paste_text", { text: token }), 1400, "clipboard token paste");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function openExtract(entry: ClipboardEntry) {
    setSelectedTokens([]);
    setExtractEntry(entry);
    setOpenActionId(null);
  }

  function closePopupDialog(dialog: "detail" | "extract") {
    setClosingDialog(dialog);
    window.setTimeout(() => {
      if (dialog === "detail") {
        setDetailEntry(null);
      } else {
        setExtractEntry(null);
        setSelectedTokens([]);
      }
      setClosingDialog(null);
    }, 150);
  }

  function handleNavigationKey(key: string, event: Pick<KeyboardEvent | React.KeyboardEvent, "preventDefault" | "stopPropagation">) {
    if (detailEntry || extractEntry) {
      return;
    }
    if (key === "Escape") {
      event.preventDefault();
      void closePopup();
      return;
    }
    if (key === "ArrowRight") {
      event.preventDefault();
      if (page < pageCount - 1) {
        showToast("→ 下一页");
      }
      changePage(page + 1);
      return;
    }
    if (key === "ArrowLeft") {
      event.preventDefault();
      if (page > 0) {
        showToast("← 上一页");
      }
      changePage(page - 1);
      return;
    }
    if (key === "ArrowDown") {
      event.preventDefault();
      event.stopPropagation();
      setOpenActionId(null);
      setSelectedIndex((index) => {
        const next = Math.min(index + 1, Math.max(0, pageEntries.length - 1));
        shouldRevealSelectedRef.current = next !== index;
        if (next !== index) {
          showToast("↓ 下一条目");
        }
        return next;
      });
      return;
    }
    if (key === "ArrowUp") {
      event.preventDefault();
      event.stopPropagation();
      setOpenActionId(null);
      setSelectedIndex((index) => {
        const next = Math.max(index - 1, 0);
        shouldRevealSelectedRef.current = next !== index;
        if (next !== index) {
          showToast("↑ 上一条目");
        }
        return next;
      });
      return;
    }
    if (key === "Enter" && selectedEntry) {
      event.preventDefault();
      void inputEntryAndClose(selectedEntry);
    }
  }

  function closeOpenActionsFromOutside(event: React.PointerEvent<HTMLElement>) {
    if (!openActionId) {
      return;
    }
    const target = event.target;
    if (!(target instanceof Element) || !target.closest(".popup-entry.actions-open")) {
      setOpenActionId(null);
    }
  }

  function sectionLabelFor(entry: ClipboardEntry, index: number) {
    const current = entry.pinnedAt ? "固定" : "最近";
    const previous = index > 0 ? (pageEntries[index - 1].pinnedAt ? "固定" : "最近") : null;
    return current === previous ? null : current;
  }

  return (
    <main className="popup-shell" onContextMenu={(event) => event.preventDefault()} onPointerDownCapture={closeOpenActionsFromOutside}>
      <div className="popup-drag-strip-wrap">
        <div
          className="popup-drag-strip"
          onMouseDown={(event) => {
            if (event.button === 0) {
              setWindowDragging(true);
              startWindowDrag();
            }
          }}
          onMouseLeave={() => setWindowDragging(false)}
          onMouseUp={() => setWindowDragging(false)}
        />
      </div>
      <section className="popup-toolbar">
        <input ref={inputRef} value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索剪贴板历史" />
        <button aria-label={pinned ? "Unpin window" : "Pin window"} className={`popup-icon-button popup-pin-button ${pinned ? "active" : ""}`} onClick={toggleWindowPinned} title={pinned ? "Unpin" : "Pin"} type="button">
          <Pin size={18} fill={pinned ? "currentColor" : "none"} />
        </button>
        <button aria-label="Open management" className="popup-icon-button popup-manage-button" onClick={() => void invoke("clipboard_open_management")} title="Open management" type="button">
          <ExternalLink size={18} />
        </button>
      </section>
      <section ref={listRef} className={`popup-list ${pageAnimating ? "page-switching" : ""}`} onScroll={() => setOpenActionId(null)}>
        {loading ? <p className="popup-empty">加载中...</p> : null}
        {error ? (
          <div className="popup-error">
            <strong>弹窗数据加载失败</strong>
            <span>{error}</span>
            <button onClick={() => void loadEntries()} type="button">重试</button>
          </div>
        ) : null}
        {!loading && !error && pageEntries.length === 0 ? <p className="popup-empty">暂无剪贴板历史</p> : null}
        {!loading && !error ? pageEntries.map((entry, index) => (
          <React.Fragment key={entry.id}>
            {sectionLabelFor(entry, index) ? <p className="popup-label">{sectionLabelFor(entry, index)}</p> : null}
            <PopupEntryCard
              actionsOpen={openActionId === entry.id}
              entry={entry}
              selected={index === selectedIndex}
              onCopy={() => void copyEntry(entry)}
              onDelete={() => void deleteEntry(entry)}
              onInput={() => void inputEntryAndClose(entry)}
              onOpenActions={(open) => setOpenActionId(open ? entry.id : null)}
              onOpenDetail={() => setDetailEntry(entry)}
              onOpenExtract={() => openExtract(entry)}
              onRegister={(node) => {
                if (node) {
                  entryRefs.current.set(entry.id, node);
                } else {
                  entryRefs.current.delete(entry.id);
                }
              }}
              onSelect={() => setSelectedIndex(index)}
              onTogglePinned={() => void togglePinned(entry)}
            />
          </React.Fragment>
        )) : null}
      </section>
      <div className="popup-page-status">{page + 1} / {pageCount}</div>
      {detailEntry ? <PopupDetailDialog closing={closingDialog === "detail"} entry={detailEntry} onClose={() => closePopupDialog("detail")} /> : null}
      {extractEntry ? (
        <PopupExtractDialog
          closing={closingDialog === "extract"}
          selectedTokens={selectedTokens}
          tokens={extractTokens}
          onClose={() => closePopupDialog("extract")}
          onConfirm={() => void copyExtractedTokens()}
          onPasteToken={(token) => void pasteTokenAndClose(token)}
          onSetSelected={(token, selected) => setSelectedTokens((current) => {
            const exists = current.includes(token);
            if (selected) {
              return exists ? current : [...current, token];
            }
            return exists ? current.filter((item) => item !== token) : current;
          })}
        />
      ) : null}
      {toast ? <div className={`popup-toast ${toastClosing ? "closing" : ""}`}>{toast}</div> : null}
    </main>
  );
}

function PopupEntryCard({
  actionsOpen,
  entry,
  onCopy,
  onDelete,
  onInput,
  onOpenActions,
  onOpenDetail,
  onOpenExtract,
  onRegister,
  onSelect,
  onTogglePinned,
  selected,
}: {
  actionsOpen: boolean;
  entry: ClipboardEntry;
  onCopy: () => void;
  onDelete: () => void;
  onInput: () => void;
  onOpenActions: (open: boolean) => void;
  onOpenDetail: () => void;
  onOpenExtract: () => void;
  onRegister: (node: HTMLElement | null) => void;
  onSelect: () => void;
  onTogglePinned: () => void;
  selected: boolean;
}) {
  const title = entry.title || entry.text.split(/\s+/).find(Boolean) || "文本条目";
  const dragRef = useRef<{ x: number; y: number; pointerId: number; moved: boolean; dragging: boolean } | null>(null);
  const suppressClickRef = useRef(false);
  const [dragOffset, setDragOffset] = useState(0);
  const [dragging, setDragging] = useState(false);
  const workspaceWidth = 106;

  function handlePointerDown(event: React.PointerEvent) {
    if (event.button !== 0) {
      return;
    }
    dragRef.current = { x: event.clientX, y: event.clientY, pointerId: event.pointerId, moved: false, dragging: false };
    setDragOffset(actionsOpen ? -workspaceWidth : 0);
    onSelect();
  }

  function handlePointerMove(event: React.PointerEvent) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    const deltaX = event.clientX - drag.x;
    const deltaY = event.clientY - drag.y;
    if (!drag.dragging && Math.abs(deltaX) < 5 && Math.abs(deltaY) < 5) {
      return;
    }
    if (!drag.dragging && Math.abs(deltaY) > Math.abs(deltaX)) {
      dragRef.current = null;
      setDragging(false);
      setDragOffset(actionsOpen ? -workspaceWidth : 0);
      return;
    }
    drag.dragging = true;
    if (!event.currentTarget.hasPointerCapture(drag.pointerId)) {
      event.currentTarget.setPointerCapture(drag.pointerId);
    }
    setDragging(true);
    drag.moved = Math.abs(deltaX) > 8;
    const base = actionsOpen ? -workspaceWidth : 0;
    setDragOffset(Math.max(-workspaceWidth, Math.min(0, base + deltaX)));
  }

  function handlePointerUp(event: React.PointerEvent) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    const delta = event.clientX - drag.x;
    const open = actionsOpen ? delta > 34 ? false : true : delta < -34;
    suppressClickRef.current = drag.moved || drag.dragging;
    dragRef.current = null;
    setDragging(false);
    setDragOffset(open ? -workspaceWidth : 0);
    onOpenActions(open);
    window.setTimeout(() => {
      suppressClickRef.current = false;
    }, 120);
  }

  function handlePointerCancel() {
    dragRef.current = null;
    setDragging(false);
    setDragOffset(actionsOpen ? -workspaceWidth : 0);
  }

  function handlePrimaryClick() {
    if (suppressClickRef.current) {
      return;
    }
    onInput();
  }

  function handleContextMenu(event: React.MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    onOpenActions(false);
    onOpenDetail();
  }

  return (
    <article
      ref={onRegister}
      className={`popup-entry ${selected ? "selected" : ""} ${actionsOpen ? "actions-open" : ""} ${dragging ? "dragging" : ""}`}
      onContextMenu={handleContextMenu}
    >
      <div
        className="popup-entry-track"
        onPointerCancel={handlePointerCancel}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        style={{ transform: `translateX(${dragging ? dragOffset : actionsOpen ? -workspaceWidth : 0}px)` }}
      >
        <div className="popup-entry-surface">
          <button className="popup-entry-main" onClick={handlePrimaryClick} type="button">
            <div className="popup-entry-title">
              {entry.pinnedAt ? <Pin size={13} fill="currentColor" /> : null}
              <strong>{title}</strong>
              <span className={`popup-entry-source ${entry.source === "derived" ? "derived" : ""}`}>{sourceLabel(entry.source)}</span>
              <span>{formatRelativeTime(entry.createdAt)}</span>
            </div>
            <p>{entry.text}</p>
            <div className="popup-entry-meta">{formatClipboardDate(entry.createdAt)} · 复制 {entry.copyCount} 次 · 使用 {entry.useCount} 次</div>
          </button>
        </div>
        <div className="popup-entry-workspace">
          <button title="分词" onClick={onOpenExtract} type="button"><Scissors size={14} /></button>
          <button className="danger" title="删除" onClick={onDelete} type="button"><Trash2 size={14} /></button>
        </div>
      </div>
      <div className="popup-entry-actions">
        <button title="复制" onClick={onCopy} type="button"><Copy size={14} /></button>
        <button title={entry.pinnedAt ? "取消固定" : "固定"} onClick={onTogglePinned} type="button"><Pin size={14} fill={entry.pinnedAt ? "currentColor" : "none"} /></button>
      </div>
    </article>
  );
}

function PopupDetailDialog({ closing, entry, onClose }: { closing: boolean; entry: ClipboardEntry; onClose: () => void }) {
  return createPortal(
    <div className={`popup-dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`popup-dialog popup-detail-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="条目详情">
        <header className="popup-dialog-header">
          <div className="popup-dialog-icon"><Info size={16} /></div>
          <div><h2>条目详情</h2></div>
          <button aria-label="关闭" className="popup-dialog-close" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <div className="popup-detail-body">
          <pre>{entry.text}</pre>
          <details>
            <summary>查看元数据</summary>
            <div className="popup-detail-meta">
              <span>来源：{sourceLabel(entry.source)}</span>
              <span>长度：{entry.text.length}</span>
              <span>创建：{formatClipboardDateTime(entry.createdAt)}</span>
              <span>复制：{formatClipboardDateTime(entry.lastCopiedAt)}</span>
              <span>使用：{formatClipboardDateTime(entry.lastUsedAt)}</span>
              <span>固定：{formatClipboardDateTime(entry.pinnedAt)}</span>
              <span>删除：{formatClipboardDateTime(entry.deletedAt)}</span>
              <span>ID：{entry.id.slice(0, 8)}</span>
            </div>
          </details>
        </div>
      </section>
    </div>,
    document.body,
  );
}

function PopupExtractDialog({
  closing,
  onClose,
  onConfirm,
  onPasteToken,
  onSetSelected,
  selectedTokens,
  tokens,
}: {
  closing: boolean;
  onClose: () => void;
  onConfirm: () => void;
  onPasteToken: (token: string) => void;
  onSetSelected: (token: string, selected: boolean) => void;
  selectedTokens: string[];
  tokens: string[];
}) {
  const dragRef = useRef<{
    anchorIndex: number;
    currentIndex: number;
    startX: number;
    startY: number;
    snapshot: Set<string>;
    applied: Set<string>;
    nextSelected: boolean;
    moved: boolean;
  } | null>(null);

  function applyTokenRange(nextIndex: number) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    drag.currentIndex = nextIndex;
    const start = Math.min(drag.anchorIndex, nextIndex);
    const end = Math.max(drag.anchorIndex, nextIndex);
    const nextRange = new Set(tokens.slice(start, end + 1));
    for (const token of drag.applied) {
      if (!nextRange.has(token)) {
        onSetSelected(token, drag.snapshot.has(token));
      }
    }
    for (const token of nextRange) {
      onSetSelected(token, drag.nextSelected);
    }
    drag.applied = nextRange;
  }

  function handleTokenPointerDown(event: React.PointerEvent<HTMLButtonElement>, index: number) {
    if (event.button !== 0) {
      return;
    }
    dragRef.current = {
      anchorIndex: index,
      currentIndex: index,
      startX: event.clientX,
      startY: event.clientY,
      snapshot: new Set(selectedTokens),
      applied: new Set(),
      nextSelected: !selectedTokens.includes(tokens[index]),
      moved: false,
    };
  }

  function handleTokenPointerMove(event: React.PointerEvent<HTMLButtonElement>) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    const deltaX = event.clientX - drag.startX;
    const deltaY = event.clientY - drag.startY;
    if (!drag.moved && Math.abs(deltaX) > 8 && Math.abs(deltaX) > Math.abs(deltaY)) {
      drag.moved = true;
      applyTokenRange(drag.currentIndex);
    }
  }

  function handleTokenPointerEnter(index: number) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    if (drag.moved || index !== drag.anchorIndex) {
      drag.moved = true;
      applyTokenRange(index);
    }
  }

  function handleTokenPointerUp(token: string) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    dragRef.current = null;
    if (!drag.moved) {
      onPasteToken(token);
    }
  }

  function handleTokenPointerCancel() {
    dragRef.current = null;
  }

  return createPortal(
    <div className={`popup-dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`popup-dialog popup-extract-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="分词提取">
        <header className="popup-dialog-header">
          <div className="popup-dialog-icon"><Scissors size={16} /></div>
          <div>
            <h2>分词提取</h2>
          </div>
          <div className="popup-dialog-inline-actions">
            <button className="popup-secondary-action" disabled={selectedTokens.length === 0} onClick={onConfirm} type="button">提取</button>
            <button aria-label="关闭" className="popup-dialog-close" onClick={onClose} type="button"><X size={13} /></button>
          </div>
        </header>
        <div className="popup-token-grid" onPointerLeave={handleTokenPointerCancel}>
          {tokens.map((token, index) => (
            <button
              className={selectedTokens.includes(token) ? "selected" : ""}
              key={`${token}-${index}`}
              onPointerCancel={handleTokenPointerCancel}
              onPointerDown={(event) => handleTokenPointerDown(event, index)}
              onPointerEnter={() => handleTokenPointerEnter(index)}
              onPointerMove={handleTokenPointerMove}
              onPointerUp={() => handleTokenPointerUp(token)}
              type="button"
            >
              {token}
            </button>
          ))}
          {tokens.length === 0 ? <p className="popup-empty">没有可提取片段</p> : null}
        </div>
      </section>
    </div>,
    document.body,
  );
}

function sourceLabel(source: string) {
  if (source === "manual") {
    return "手动";
  }
  if (source === "derived") {
    return "分词提取";
  }
  if (source === "system" || source === "clipboard") {
    return "复制";
  }
  return "复制";
}

function formatClipboardDate(value?: number | null) {
  if (!value) {
    return "未记录";
  }
  const date = new Date(Number(value));
  const y = date.getFullYear();
  const m = `${date.getMonth() + 1}`.padStart(2, "0");
  const d = `${date.getDate()}`.padStart(2, "0");
  const hh = `${date.getHours()}`.padStart(2, "0");
  const mm = `${date.getMinutes()}`.padStart(2, "0");
  return `${y}/${m}/${d} ${hh}:${mm}`;
}

function formatClipboardDateTime(value?: number | null) {
  return value ? new Date(Number(value)).toLocaleString() : "未记录";
}

function formatRelativeTime(value?: number | null) {
  if (!value) {
    return "";
  }
  const diff = Date.now() - Number(value);
  if (diff < 60_000) {
    return "刚刚";
  }
  if (diff < 3_600_000) {
    return `${Math.max(1, Math.round(diff / 60_000))} 分钟前`;
  }
  if (diff < 86_400_000) {
    return `${Math.max(1, Math.round(diff / 3_600_000))} 小时前`;
  }
  return formatClipboardDate(value).slice(0, 10);
}

type WordSegmenter = {
  segment(input: string): Iterable<{ segment: string; isWordLike?: boolean }>;
};

type WordSegmenterConstructor = new (locale: string, options: { granularity: "word" }) => WordSegmenter;

function extractClipboardTokens(text: string) {
  const tokens: string[] = [];
  const seen = new Set<string>();
  const addToken = (token: string) => {
    const normalized = token.trim();
    if (!normalized || seen.has(normalized)) {
      return;
    }
    seen.add(normalized);
    tokens.push(normalized);
  };

  const structuredMatches = text.match(/[a-zA-Z]+:\/\/[^\s"'<>]+|[a-zA-Z]:\\[^\r\n]+|\\\\[^\s]+|[\w.-]+@[\w.-]+\.[A-Za-z]{2,}|[\w.-]+\.[A-Za-z]{2,}(?:\/[^\s]*)?|--?[A-Za-z][\w-]*|v?\d+(?:\.\d+){1,}|[A-Za-z0-9_./\\:-]{4,}/g) ?? [];
  structuredMatches.forEach(addToken);

  const segmenterCtor = (Intl as unknown as { Segmenter?: WordSegmenterConstructor }).Segmenter;
  const segmenter = segmenterCtor ? new segmenterCtor("zh-CN", { granularity: "word" }) : null;
  const chineseRuns = text.match(/[\u3400-\u9fff]+/g) ?? [];
  for (const run of chineseRuns) {
    if (segmenter) {
      for (const item of segmenter.segment(run)) {
        if (item.isWordLike !== false && item.segment.length >= 2) {
          addToken(item.segment);
        }
      }
    } else if (run.length <= 8) {
      addToken(run);
    } else {
      for (let index = 0; index < run.length - 1; index += 2) {
        addToken(run.slice(index, index + 2));
      }
    }
  }

  return tokens.slice(0, 80);
}

ReactDOM.createRoot(document.getElementById("popup-root") as HTMLElement).render(
  <React.StrictMode>
    <PopupApp />
  </React.StrictMode>,
);
