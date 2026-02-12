import { Suspense, lazy, useEffect } from 'react'
import { emit } from '@tauri-apps/api/event'
import { useConfig } from './hooks/useConfig';
import { useTheme } from './hooks/useTheme';

const MainWindow = lazy(() =>
  import('./components/MainWindow').then(module => ({ default: module.MainWindow }))
)

const AppBootFallback = ({ message }: { message: string }) => (
  <div className="w-full h-screen flex items-center justify-center bg-[var(--bg-primary)]">
    <div className="px-5 py-4 rounded-xl border border-[var(--glass-border)] bg-[var(--bg-secondary)] text-center">
      <div className="text-sm font-semibold text-[var(--text-primary)]">Resh</div>
      <div className="text-xs text-[var(--text-secondary)] mt-1">{message}</div>
    </div>
  </div>
)

const AppReadySignal = () => {
  useEffect(() => {
    window.dispatchEvent(new Event('resh-app-ready'))
    void emit('resh-app-ready').catch(() => {})
  }, [])

  return null
}

function App() {
  const { config, loading } = useConfig()
  const theme = config?.general.theme

  useTheme(theme)

  if (loading) {
    return <AppBootFallback message="Loading workspace..." />
  }

  return (
    <Suspense fallback={<AppBootFallback message="Loading interface..." />}>
      <AppReadySignal />
      <div className="w-full h-screen flex flex-col">
        <MainWindow />
      </div>
    </Suspense>
  )
}

export default App;
