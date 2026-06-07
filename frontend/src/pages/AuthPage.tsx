import { useCallback, useEffect, useRef, useState } from "react"
import { Check, Copy, Loader2, RefreshCw, ShieldCheck } from "lucide-react"
import { api, ApiError, type AuthRefreshResult } from "@/api/client"
import { Button } from "@/components/ui/button"

const TURNSTILE_SCRIPT_ID = "mem-turnstile-script"

declare global {
  interface Window {
    turnstile?: {
      render: (
        target: HTMLElement | string,
        options: {
          sitekey: string
          theme?: "auto" | "light" | "dark"
          appearance?: "always" | "execute" | "interaction-only"
          callback?: (token: string) => void
          "error-callback"?: () => void
          "expired-callback"?: () => void
        },
      ) => string
      execute: (widgetId: string) => void
      reset: (widgetId: string) => void
      remove?: (widgetId: string) => void
    }
  }
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof ApiError ? error.message : fallback
}

function formatRemaining(seconds: number) {
  const safeSeconds = Math.max(0, seconds)
  const minutes = Math.floor(safeSeconds / 60)
  return `${minutes}:${String(safeSeconds % 60).padStart(2, "0")}`
}

export function AuthPage() {
  const widgetRef = useRef<HTMLDivElement | null>(null)
  const widgetIdRef = useRef<string | null>(null)
  const [authToken, setAuthToken] = useState("")
  const [expiresAt, setExpiresAt] = useState<number | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [widgetReady, setWidgetReady] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState("")
  const [turnstileSiteKey, setTurnstileSiteKey] = useState("")

  const clearToken = useCallback(() => {
    setAuthToken("")
    setExpiresAt(null)
    setCopied(false)
  }, [])

  const resetTurnstile = useCallback(() => {
    const widgetId = widgetIdRef.current
    if (widgetId) window.turnstile?.reset(widgetId)
  }, [])

  const acceptAuthResult = useCallback((result: AuthRefreshResult) => {
    setAuthToken(result.auth_token)
    setExpiresAt(result.expires_at)
  }, [])

  const forceRefreshToken = useCallback(() => {
    const widgetId = widgetIdRef.current
    if (!widgetId || !window.turnstile) {
      setError("Turnstile 未就绪")
      return
    }
    setRefreshing(true)
    setError("")
    setCopied(false)
    window.turnstile.reset(widgetId)
    window.turnstile.execute(widgetId)
  }, [])

  const refreshToken = useCallback(
    async (turnstileToken: string) => {
      setRefreshing(true)
      setError("")
      clearToken()
      try {
        const result = await api.auth.refresh(turnstileToken)
        acceptAuthResult(result)
      } catch (caught) {
        setError(errorMessage(caught, "授权失败"))
        resetTurnstile()
      } finally {
        setRefreshing(false)
      }
    },
    [acceptAuthResult, clearToken, resetTurnstile],
  )

  useEffect(() => {
    let cancelled = false
    api.auth
      .status()
      .then((status) => {
        if (cancelled) return
        const siteKey = status.turnstile_site_key?.trim()
        if (!siteKey) {
          setError("Turnstile 未配置")
          return
        }
        setTurnstileSiteKey(siteKey)
      })
      .catch((caught) => {
        if (!cancelled) setError(errorMessage(caught, "授权状态读取失败"))
      })
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (!turnstileSiteKey) return
    let cancelled = false
    const renderWidget = () => {
      if (cancelled || widgetIdRef.current || !widgetRef.current || !window.turnstile) return
      widgetIdRef.current = window.turnstile.render(widgetRef.current, {
        sitekey: turnstileSiteKey,
        theme: "auto",
        appearance: "interaction-only",
        callback: refreshToken,
        "error-callback": () => {
          clearToken()
          setRefreshing(false)
          setError("Turnstile 验证失败")
        },
        "expired-callback": () => {
          clearToken()
          setRefreshing(false)
          setError("")
        },
      })
      setWidgetReady(true)
    }

    let script = document.getElementById(TURNSTILE_SCRIPT_ID) as HTMLScriptElement | null
    if (!script) {
      script = document.createElement("script")
      script.id = TURNSTILE_SCRIPT_ID
      script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit"
      script.async = true
      script.defer = true
      script.onload = renderWidget
      document.head.appendChild(script)
    }
    renderWidget()
    const timer = window.setInterval(renderWidget, 100)

    return () => {
      cancelled = true
      window.clearInterval(timer)
      if (widgetIdRef.current) window.turnstile?.remove?.(widgetIdRef.current)
      widgetIdRef.current = null
    }
  }, [clearToken, refreshToken, turnstileSiteKey])

  useEffect(() => {
    if (!expiresAt) return
    const timer = window.setInterval(() => {
      const currentTime = Math.floor(Date.now() / 1000)
      setNow(currentTime)
      if (currentTime >= expiresAt) {
        clearToken()
        resetTurnstile()
      }
    }, 1000)
    return () => window.clearInterval(timer)
  }, [clearToken, expiresAt, resetTurnstile])

  useEffect(() => {
    if (!authToken) return
    const timer = window.setInterval(async () => {
      try {
        const status = await api.auth.status()
        if (!status.valid || status.expires_at !== expiresAt) {
          clearToken()
          resetTurnstile()
        }
      } catch {
        clearToken()
        resetTurnstile()
      }
    }, 4000)
    return () => window.clearInterval(timer)
  }, [authToken, clearToken, expiresAt, resetTurnstile])

  const copyToken = async () => {
    if (!authToken) return
    try {
      await navigator.clipboard.writeText(authToken)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      setError("复制失败")
    }
  }

  const remaining = expiresAt ? expiresAt - now : 0

  return (
    <div className="p-4 sm:p-6 max-w-3xl">
      <div className="mb-5 flex items-center gap-3">
        <ShieldCheck className="h-5 w-5 text-muted-foreground" />
        <h1 className="text-lg font-semibold">Init 授权</h1>
      </div>

      <div className="rounded-md border bg-card p-4 sm:p-5">
        <div ref={widgetRef} className="min-h-[70px]" />
        {!widgetReady && !error && (
          <div className="flex h-[70px] items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            加载中
          </div>
        )}
        {refreshing && (
          <div className="mt-3 flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            签发中
          </div>
        )}
        {error && <div className="mt-3 text-sm text-destructive">{error}</div>}
      </div>

      {authToken && expiresAt && (
        <div className="mt-4 rounded-md border bg-card p-4 sm:p-5">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div className="text-sm font-medium">auth_token</div>
            <div className="text-sm tabular-nums text-muted-foreground">
              {formatRemaining(remaining)}
            </div>
          </div>
          <pre className="max-h-40 overflow-auto rounded-md border bg-muted/50 p-3 text-xs break-all whitespace-pre-wrap">
            {authToken}
          </pre>
          <div className="mt-3 flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={forceRefreshToken}
              disabled={!widgetReady || refreshing}
              className="gap-2"
            >
              <RefreshCw className="h-4 w-4" />
              重新获取
            </Button>
            <Button type="button" size="sm" onClick={copyToken} className="gap-2">
              {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
              {copied ? "已复制" : "复制"}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
