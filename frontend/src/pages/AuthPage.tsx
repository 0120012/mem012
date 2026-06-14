import { useCallback, useEffect, useState } from "react"
import { Check, Copy, Loader2, RefreshCw, ShieldCheck } from "lucide-react"
import { api, ApiError, type AuthRefreshResult } from "@/api/client"
import { Button } from "@/components/ui/button"

function errorMessage(error: unknown, fallback: string) {
  return error instanceof ApiError ? error.message : fallback
}

function formatRemaining(seconds: number) {
  const safeSeconds = Math.max(0, seconds)
  const minutes = Math.floor(safeSeconds / 60)
  return `${minutes}:${String(safeSeconds % 60).padStart(2, "0")}`
}

export function AuthPage() {
  const [authToken, setAuthToken] = useState("")
  const [expiresAt, setExpiresAt] = useState<number | null>(null)
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  const [refreshing, setRefreshing] = useState(false)
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState("")

  const clearToken = useCallback(() => {
    setAuthToken("")
    setExpiresAt(null)
    setCopied(false)
  }, [])

  const acceptAuthResult = useCallback((result: AuthRefreshResult) => {
    setAuthToken(result.auth_token)
    setExpiresAt(result.expires_at)
  }, [])

  const refreshToken = useCallback(
    async () => {
      setRefreshing(true)
      setError("")
      clearToken()
      try {
        const result = await api.auth.refresh()
        acceptAuthResult(result)
      } catch (caught) {
        setError(errorMessage(caught, "授权失败"))
      } finally {
        setRefreshing(false)
      }
    },
    [acceptAuthResult, clearToken],
  )

  const forceRefreshToken = useCallback(() => {
    void refreshToken()
  }, [refreshToken])

  useEffect(() => {
    if (!expiresAt) return
    const timer = window.setInterval(() => {
      const currentTime = Math.floor(Date.now() / 1000)
      setNow(currentTime)
      if (currentTime >= expiresAt) {
        clearToken()
      }
    }, 1000)
    return () => window.clearInterval(timer)
  }, [clearToken, expiresAt])

  useEffect(() => {
    if (!authToken) return
    const timer = window.setInterval(async () => {
      try {
        const status = await api.auth.status()
        if (!status.valid || status.expires_at !== expiresAt) {
          clearToken()
        }
      } catch {
        clearToken()
      }
    }, 4000)
    return () => window.clearInterval(timer)
  }, [authToken, clearToken, expiresAt])

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
        <Button type="button" onClick={forceRefreshToken} disabled={refreshing} className="gap-2">
          {refreshing ? <Loader2 className="h-4 w-4 animate-spin" /> : <RefreshCw className="h-4 w-4" />}
          获取 auth_token
        </Button>
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
              disabled={refreshing}
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
