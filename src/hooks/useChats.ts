import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export type ChatThread = {
  id: string;
  external_id: string;
  display_name: string;
  is_group: boolean;
  message_count: number;
  latest_at: string | null;
  latest_preview: string;
};

export type ChatMessageMedia = {
  id: string;
  media_type: "image" | "video" | "audio";
  original_path: string;
  overlay_path: string | null;
  thumbnail_path: string | null;
  playback_path: string | null;
};

export type ChatMessage = {
  id: string;
  sender: string;
  sender_label: string;
  is_me: boolean;
  body: string | null;
  sent_at: string;
  message_type: string;
  media: ChatMessageMedia[];
};

export function useChatThreads() {
  const [threads, setThreads] = useState<ChatThread[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ChatThread[]>("list_chat_threads")
      .then(setThreads)
      .catch((err) => setError(String(err)))
      .finally(() => setIsLoading(false));
  }, []);

  return { threads, isLoading, error };
}

export function useChatMessages(threadId: string | null) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!threadId) {
      setMessages([]);
      return;
    }
    setIsLoading(true);
    setError(null);
    try {
      const result = await invoke<ChatMessage[]>("list_chat_messages", { threadId });
      setMessages(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, [threadId]);

  useEffect(() => {
    load();
  }, [load]);

  return { messages, isLoading, error };
}

// Avatar initials from a display name, mirroring the reference app's
// text-only avatars (no profile images in a Snapchat export).
export function avatarInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) {
    return "?";
  }
  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase();
  }
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

const GROUP_TONES = [
  { label: "text-cyan-300", pole: "bg-cyan-400" },
  { label: "text-lime-300", pole: "bg-lime-400" },
  { label: "text-fuchsia-300", pole: "bg-fuchsia-400" },
  { label: "text-amber-300", pole: "bg-amber-400" },
  { label: "text-violet-300", pole: "bg-violet-400" },
  { label: "text-emerald-300", pole: "bg-emerald-400" },
];
const PRIVATE_ME_TONE = { label: "text-rose-300", pole: "bg-rose-400" };
const PRIVATE_OTHER_TONE = { label: "text-sky-300", pole: "bg-sky-400" };

function hashSenderLabel(label: string): number {
  let hash = 0;
  for (const char of label) {
    hash = (hash * 31 + char.charCodeAt(0)) | 0;
  }
  return Math.abs(hash);
}

// Picks a deterministic accent color per sender: binary me/them in a 1:1
// chat, or a hash-based rotation through fixed tones in a group.
export function senderTone(message: ChatMessage, isGroup: boolean): { label: string; pole: string } {
  if (!isGroup) {
    return message.is_me ? PRIVATE_ME_TONE : PRIVATE_OTHER_TONE;
  }
  return GROUP_TONES[hashSenderLabel(message.sender_label) % GROUP_TONES.length];
}

const AVATAR_GRADIENTS = [
  "linear-gradient(135deg,#565a86,#2b2741)",
  "linear-gradient(135deg,#6b4a5c,#2b2741)",
  "linear-gradient(135deg,#42624f,#2b2741)",
  "linear-gradient(135deg,#7d5f3c,#2b2741)",
  "linear-gradient(135deg,#3a5c6b,#2b2741)",
  "linear-gradient(135deg,#8a6446,#2b2741)",
];

function hashString(value: string): number {
  let hash = 0;
  for (const char of value) {
    hash = (hash * 31 + char.charCodeAt(0)) | 0;
  }
  return Math.abs(hash);
}

// A deterministic muted gradient per thread, so conversations stay visually
// distinct in the list without reaching for saturated per-sender colors.
export function avatarGradient(seed: string): string {
  return AVATAR_GRADIENTS[hashString(seed) % AVATAR_GRADIENTS.length];
}
