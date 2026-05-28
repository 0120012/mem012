import { Suspense, lazy } from "react"
import { Routes, Route, Navigate } from "react-router-dom"
import { AuthProvider } from "@/auth/AuthProvider"
import { ProtectedRoute } from "@/auth/ProtectedRoute"
import { ProjectGuard } from "@/auth/ProjectGuard"
import { Layout } from "@/components/Layout"

const LoginPage = lazy(() => import("@/pages/LoginPage").then((page) => ({ default: page.LoginPage })))
const MemoriesPage = lazy(() => import("@/pages/MemoriesPage").then((page) => ({ default: page.MemoriesPage })))
const ChangesPage = lazy(() => import("@/pages/ChangesPage").then((page) => ({ default: page.ChangesPage })))
const GraphPage = lazy(() => import("@/pages/GraphPage").then((page) => ({ default: page.GraphPage })))
const AuthPage = lazy(() => import("@/pages/AuthPage").then((page) => ({ default: page.AuthPage })))

function PageFallback() {
  // Why：页面按路由拆包后需要一个稳定高度占位，避免加载瞬间布局跳动。
  return <div className="h-24" />
}

export default function App() {
  return (
    <AuthProvider>
      <Suspense fallback={<PageFallback />}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route element={<ProtectedRoute />}>
            <Route element={<Layout />}>
              <Route path="/auth" element={<AuthPage />} />
              <Route element={<ProjectGuard />}>
                <Route path="/memories" element={<MemoriesPage />} />
                <Route path="/changes" element={<ChangesPage />} />
                <Route path="/graph" element={<GraphPage />} />
              </Route>
            </Route>
            <Route path="/" element={<Navigate to="/memories" replace />} />
            <Route path="*" element={<Navigate to="/memories" replace />} />
          </Route>
        </Routes>
      </Suspense>
    </AuthProvider>
  )
}
