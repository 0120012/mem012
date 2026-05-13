import { useState } from "react"
import { useNavigate, Navigate } from "react-router-dom"
import { useAuth } from "@/auth/AuthProvider"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/card"

export function LoginPage() {
  const { login, isLoggedIn, isLoading } = useAuth()
  const navigate = useNavigate()
  const [key, setKey] = useState("")
  const [error, setError] = useState("")
  const [loading, setLoading] = useState(false)

  if (!isLoading && isLoggedIn) {
    return <Navigate to="/memories" replace />;
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setLoading(true)
    const result = await login(key)
    setLoading(false)
    if (result.success) {
      navigate("/memories")
    } else {
      setError(result.error || "认证失败")
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>登录</CardTitle>
          <CardDescription>输入访问密钥以继续</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="flex flex-col gap-4">
            <div className="flex flex-col gap-2">
              <Label htmlFor="key">密钥</Label>
              <Input
                id="key"
                type="password"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder="请输入密钥"
                disabled={loading}
                autoFocus
              />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
            <Button type="submit" disabled={loading}>
              {loading ? "验证中..." : "登录"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}