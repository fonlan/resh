import './App.css';
import { MainWindow } from './components/MainWindow';
import { useConfig } from './hooks/useConfig';
import { useTheme } from './hooks/useTheme';

function App() {
  const { config } = useConfig();
  const theme = config?.general.theme;

  useTheme(theme);

  return (
    <div className="app-container">
      <MainWindow />
    </div>
  );
}

export default App;
