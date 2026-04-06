import { useCallback, useMemo, useSyncExternalStore } from "react";
import { api } from "@/lib/api";

export interface User {
  id: string;
  email: string;
  tier?: string;
}

interface JwtPayload {
  sub?: string;
  email?: string;
  tier?: string;
  exp?: number;
}

interface AuthResponse {
  access_token: string;
  user?: User;
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

function parseJwt(token: string): JwtPayload | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;
    const payload = parts[1].replace(/-/g, "+").replace(/_/g, "/");
    return JSON.parse(atob(payload)) as JwtPayload;
  } catch {
    return null;
  }
}

function isTokenExpired(token: string): boolean {
  const claims = parseJwt(token);
  if (!claims?.exp) return false;
  return Date.now() >= claims.exp * 1000;
}

function userFromToken(token: string): User | null {
  const claims = parseJwt(token);
  if (!claims) return null;
  return {
    id: claims.sub ?? "",
    email: claims.email ?? "",
    tier: claims.tier,
  };
}

function storeAuth(token: string, serverUser?: User) {
  localStorage.setItem(TOKEN_KEY, token);
  const user = serverUser ?? userFromToken(token);
  if (user) localStorage.setItem(USER_KEY, JSON.stringify(user));
  emitChange();
}

function clearAuth() {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(USER_KEY);
  emitChange();
}

export function useAuth() {
  const token = useSyncExternalStore(subscribe, getSnapshot);

  const user: User | null = useMemo(() => {
    if (!token) return null;
    const raw = localStorage.getItem(USER_KEY);
    if (raw) {
      try {
        return JSON.parse(raw) as User;
      } catch {
        /* fall through to JWT parsing */
      }
    }
    return userFromToken(token);
  }, [token]);

  const isAuthenticated = useMemo(() => {
    if (!token) return false;
    return !isTokenExpired(token);
  }, [token]);

  const login = useCallback(async (email: string, password: string) => {
    const data = await api.post<AuthResponse>("/auth/login", {
      email,
      password,
    });
    storeAuth(data.access_token, data.user);
  }, []);

  const signup = useCallback(async (email: string, password: string) => {
    const data = await api.post<AuthResponse>("/auth/signup", {
      email,
      password,
    });
    storeAuth(data.access_token, data.user);
  }, []);

  const logout = useCallback(() => {
    clearAuth();
    window.location.href = "/login";
  }, []);

  return { user, isAuthenticated, login, signup, logout } as const;
}
