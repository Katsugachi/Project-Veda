import type { Chat, ChatMessage } from "./types";

const STORAGE_KEY = "veda:chats";
const MAX_CHATS = 100;

function storageGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storageSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage unavailable (some embedded webviews); keep the in-memory store.
  }
}

export function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return `id-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }
}

export function createChat(): Chat {
  const now = Date.now();
  return { id: newId(), title: "New chat", createdAt: now, updatedAt: now, messages: [] };
}

/** Derives a readable title from the first thing the user actually said. */
export function deriveTitle(message: string): string {
  const clean = message.replace(/\s+/g, " ").trim();
  if (!clean) return "New chat";
  return clean.length > 48 ? `${clean.slice(0, 47)}…` : clean;
}

function isMessage(value: unknown): value is ChatMessage {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<ChatMessage>;
  return (
    typeof candidate.id === "string" &&
    (candidate.role === "user" || candidate.role === "assistant") &&
    typeof candidate.content === "string" &&
    typeof candidate.createdAt === "number"
  );
}

/** Persisted data is untrusted: anything malformed is dropped, never thrown. */
export function loadChats(): Chat[] {
  const raw = storageGet(STORAGE_KEY);
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is Chat => {
        if (!entry || typeof entry !== "object") return false;
        const chat = entry as Partial<Chat>;
        return typeof chat.id === "string" && Array.isArray(chat.messages);
      })
      .map((chat) => ({
        id: chat.id,
        title: typeof chat.title === "string" && chat.title.trim() ? chat.title : "New chat",
        createdAt: typeof chat.createdAt === "number" ? chat.createdAt : Date.now(),
        updatedAt: typeof chat.updatedAt === "number" ? chat.updatedAt : Date.now(),
        // A reply that was still streaming when the app closed is not resumable,
        // so the flag is cleared on load instead of leaving a stuck caret.
        messages: chat.messages.filter(isMessage).map((message) => ({ ...message, streaming: false })),
      }))
      .slice(0, MAX_CHATS);
  } catch {
    return [];
  }
}

export function saveChats(chats: Chat[]): void {
  const persistable = chats
    .filter((chat) => chat.messages.length > 0)
    .slice(0, MAX_CHATS)
    .map((chat) => ({
      ...chat,
      messages: chat.messages.map(({ streaming: _streaming, ...message }) => message),
    }));
  storageSet(STORAGE_KEY, JSON.stringify(persistable));
}

/** Groups chats into the buckets shown in the sidebar. */
export function groupChats(chats: Chat[], now = Date.now()): { label: string; chats: Chat[] }[] {
  const day = 24 * 60 * 60 * 1000;
  const buckets: { label: string; chats: Chat[] }[] = [
    { label: "Today", chats: [] },
    { label: "Previous 7 days", chats: [] },
    { label: "Older", chats: [] },
  ];
  const sorted = [...chats].sort((left, right) => right.updatedAt - left.updatedAt);
  for (const chat of sorted) {
    const age = now - chat.updatedAt;
    if (age < day) buckets[0].chats.push(chat);
    else if (age < 7 * day) buckets[1].chats.push(chat);
    else buckets[2].chats.push(chat);
  }
  return buckets.filter((bucket) => bucket.chats.length > 0);
}
