import { MainWindow } from './components/MainWindow';
import { useConfig } from './hooks/useConfig';
import { useTheme } from './hooks/useTheme';

function App() {
  const { config } = useConfig();
  const theme = config?.general.theme;

  useTheme(theme);

  return (
    <div className="w-full h-screen flex flex-col">
      <MainWindow />
    </div>
  );
}

export default App;
