import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export type FriendPreview = {
  Username: string;
  "Display Name": string;
  "Creation Timestamp"?: string;
};

export type ProfileSnapshotData = {
  account: {
    username: string | null;
    display_name: string | null;
    created_at: string | null;
    country: string | null;
    registration_ip: string | null;
    in_app_language: string | null;
  };
  friends: {
    friends_count: number;
    blocked_count: number;
    deleted_count: number;
    top_friends: FriendPreview[];
  };
  ranking: {
    snapscore: number | null;
    total_friends: number | null;
  };
  engagement: {
    application_opens: number | null;
    story_views: number | null;
    snap_views: number | null;
    chats_sent: number | null;
    chats_viewed: number | null;
    direct_snaps_created: number | null;
  };
  bitmoji: {
    avatar_gender: string | null;
    app_open_count: number | null;
    outfit_save_count: number | null;
    share_count: number | null;
    account_created_at: string | null;
  };
};

export type ProfileSnapshot = {
  generated_at: string | null;
  snapshot: ProfileSnapshotData;
  memory_count: number;
};

export function useProfile() {
  const [profile, setProfile] = useState<ProfileSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ProfileSnapshot | null>("get_profile_snapshot")
      .then(setProfile)
      .catch((err) => setError(String(err)))
      .finally(() => setIsLoading(false));
  }, []);

  return { profile, isLoading, error };
}
