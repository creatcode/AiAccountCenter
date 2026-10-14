import { createContext, useCallback, useContext, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Loader2, ShieldCheck } from "lucide-react";

import { AppIconMark } from "@/components/product-marks";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import * as api from "@/lib/api";

/**
 * 管理面登录门禁（仅 Web 形态）。
 *
 * 服务端已对除公开路由外的 `/api` 一律要求会话，这里只负责让浏览器先拿到会话：
 * 未登录时整页替换为登录/初始化表单，登录成功后再挂载应用。桌面宿主与演示模式
 * 不走这条路径——它们的可信边界是 Tauri 窗口，不是 Cookie。
 */

type GateState = "checking" | "needLogin" | "ready";

interface AuthGateContextValue {
  signOut: () => void;
}

const AuthGateContext = createContext<AuthGateContextValue>({ signOut: () => {} });

/** 供侧栏「退出登录」等入口调用；非 Web 形态下是空操作。 */
export function useAuthGate(): AuthGateContextValue {
  return useContext(AuthGateContext);
}

export function AuthGate({ children }: { children: ReactNode }) {
  /** 桌面宿主与演示模式没有管理面会话，直接放行。 */
  const gated = api.isWebui() && !api.isDemoMode();
  const [state, setState] = useState<GateState>(gated ? "checking" : "ready");
  const [status, setStatus] = useState<{ initialized: boolean; initCodeRequired: boolean } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await api.getAuthStatus();
      setStatus(next);
      setState(next.authenticated ? "ready" : "needLogin");
    } catch {
      // 服务不可达时不要把人挡在登录页：让应用按原有错误处理展示问题。
      setState("ready");
    }
  }, []);

  useEffect(() => {
    if (!gated) return;
    void refresh();
    // 会话在用了一段时间后过期/被撤销：任何业务请求收到 401 都把人带回登录页，
    // 而不是让每个页面各自弹一句「未登录或会话已过期」。
    const onUnauthorized = () => setState("needLogin");
    window.addEventListener(api.UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(api.UNAUTHORIZED_EVENT, onUnauthorized);
  }, [gated, refresh]);

  const signOut = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      setState("needLogin");
      setStatus(null);
      await refresh();
    }
  }, [refresh]);

  if (!gated || state === "ready") {
    return (
      <AuthGateContext.Provider value={{ signOut }}>
        {children}
      </AuthGateContext.Provider>
    );
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4 py-10">
      {state === "checking" ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          正在检查登录状态…
        </div>
      ) : (
        <AuthPanel
          initialized={status?.initialized ?? true}
          initCodeRequired={status?.initCodeRequired ?? false}
          onDone={refresh}
        />
      )}
    </div>
  );
}

function AuthPanel({
  initialized,
  initCodeRequired,
  onDone,
}: {
  initialized: boolean;
  initCodeRequired: boolean;
  onDone: () => Promise<void>;
}) {
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (initialized) {
        await api.login(password);
      } else {
        await api.initAdmin(code, password);
      }
      await onDone();
    } catch (cause) {
      setError(api.asError(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="w-full max-w-sm rounded-xl py-0 shadow-none">
      <CardContent className="px-6 py-7">
        <div className="flex items-center gap-2.5">
          <AppIconMark size={32} className="drop-shadow-sm" />
          <div className="text-[15px] font-medium tracking-[-0.02em]">AiAccountCenter</div>
        </div>

        <div className="mt-5 flex items-center gap-1.5 text-[13px] font-medium">
          <ShieldCheck className="size-4 text-muted-foreground" aria-hidden="true" />
          {initialized ? "登录管理面" : "首次初始化"}
        </div>
        <p className="mt-1.5 text-xs leading-5 text-muted-foreground">
          {initialized
            ? "输入管理员密码以继续。"
            : initCodeRequired
              ? "请输入服务启动时在本机控制台打印的一次性初始化码，并设置管理员密码。"
              : "请在服务所在机器的控制台重启服务，取得一次性初始化码后再来设置。"}
        </p>

        <form className="mt-5 space-y-3" onSubmit={onSubmit}>
          {!initialized && (
            <div className="space-y-1.5">
              <Label htmlFor="auth-code" className="text-[13px]">
                初始化码
              </Label>
              <Input
                id="auth-code"
                value={code}
                onChange={(event) => setCode(event.target.value)}
                autoComplete="off"
                autoCapitalize="characters"
                spellCheck={false}
                placeholder="启动日志中的 12 位码"
                required
              />
            </div>
          )}
          <div className="space-y-1.5">
            <Label htmlFor="auth-password" className="text-[13px]">
              管理员密码
            </Label>
            <Input
              id="auth-password"
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              autoComplete={initialized ? "current-password" : "new-password"}
              placeholder={initialized ? "" : "至少 8 位"}
              required
            />
          </div>

          {error && (
            <p className="text-xs leading-5 text-destructive" role="alert">
              {error}
            </p>
          )}

          <Button type="submit" className="w-full" disabled={busy}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden="true" />}
            {initialized ? "登录" : "设置并登录"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
