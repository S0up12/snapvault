import { useCallback, useEffect, useRef, useState } from "react";

import { invokeLogged } from "../lib/ipc";

export type ChatSearchHit = {
  message_id: string;
  thread_id: string;
  thread_title: string | null;
  sent_at: string;
  is_sender: boolean;
  sender_label: string;
  snippet: string;
};

type ChatSearchPage = { hits: ChatSearchHit[]; total: number; total_capped: boolean; has_more: boolean };

const DEBOUNCE_MS = 250;

// Debounced full-text message search. Never logs the query (invokeLogged only
// logs arg names). Stale responses are dropped via a generation counter.
export function useChatSearch(threadId: string | null) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<ChatSearchHit[]>([]);
  const [total, setTotal] = useState(0);
  const [totalCapped, setTotalCapped] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const loadingMore = useRef(false);

  const active = query.trim().length > 0;

  useEffect(() => {
    const gen = ++generation.current;
    loadingMore.current = false;
    setError(null);
    if (!query.trim()) {
      setHits([]);
      setTotal(0);
      setTotalCapped(false);
      setHasMore(false);
      setIsLoading(false);
      return;
    }
    setIsLoading(true);
    const timer = setTimeout(() => {
      invokeLogged<ChatSearchPage>("search_chat_messages", { query, threadId: threadId ?? undefined })
        .then((page) => {
          if (gen !== generation.current) return;
          setHits(page.hits);
          setTotal(page.total);
          setTotalCapped(page.total_capped);
          setHasMore(page.has_more);
        })
        .catch((err) => gen === generation.current && setError(String(err)))
        .finally(() => gen === generation.current && setIsLoading(false));
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, threadId]);

  const loadMore = useCallback(async () => {
    if (!hasMore || loadingMore.current || !query.trim()) return;
    const gen = generation.current;
    loadingMore.current = true;
    setIsLoading(true);
    try {
      const page = await invokeLogged<ChatSearchPage>("search_chat_messages", {
        query,
        threadId: threadId ?? undefined,
        offset: hits.length,
      });
      if (gen !== generation.current) return;
      setHits((prev) => {
        const seen = new Set(prev.map((h) => h.message_id));
        return [...prev, ...page.hits.filter((h) => !seen.has(h.message_id))];
      });
      setTotal(page.total);
      setTotalCapped(page.total_capped);
      setHasMore(page.has_more);
    } catch (err) {
      if (gen === generation.current) setError(String(err));
    } finally {
      if (gen === generation.current) {
        loadingMore.current = false;
        setIsLoading(false);
      }
    }
  }, [hasMore, query, threadId, hits.length]);

  return { query, setQuery, active, hits, total, totalCapped, hasMore, isLoading, error, loadMore };
}
