import { useCallback, useMemo, useSyncExternalStore } from "react";
import { api } from "@/lib/api";

interface User {
  id: string;
  email: string;
}

interface AuthResponse {
  access_token: string;
  user: User;
}

const TOKEN_KEY = "token";
const USER_KEY = "user";

let listeners: Array<() => void> = [];
function emitChange() {
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners = [...listeners, listener];
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

function getSnapshot(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function useAuth() {
  const token = useSyncExternalStore(subscribe, getSnapshot);

  const user: User | null = useMemo(() => {
    const raw = localStorage.getItem(USER_KEY);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as User;
    } catch {
      return null;
    }
  }, [token]);

  const isAuthenticated = token !== null;

  const login = useCallback(async (email: string, password: string) => {
    const data = await api.post<AuthResponse>("/auth/login", {
      email,
      password,
    });
    localStorage.setItem(TOKEN_KEY, data.access_token);
    localStorage.setItem(USER_KEY, JSON.stringify(data.user));
    emitChange();
  }, []);

  const signup = useCallback(async (email: string, password: string) => {
    const data = await api.post<AuthResponse>("/auth/signup", {
      email,
      password,
    });
    localStorage.setItem(TOKEN_KEY, data.access_token);
    localStorage.setItem(USER_KEY, JSON.stringify(data.user));
    emitChange();
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(USER_KEY);
    emitChange();
  }, []);

  return { user, isAuthenticated, login, signup, logout } as const;
}
