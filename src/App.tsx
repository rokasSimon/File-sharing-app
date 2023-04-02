import React from 'react';
import './App.css';

import RemoveRoundedIcon from '@mui/icons-material/RemoveRounded';
import CloseRoundedIcon from '@mui/icons-material/CloseRounded';
import CropSquareRounded from '@mui/icons-material/CropSquareRounded';

import { appWindow } from "@tauri-apps/api/window";
import Menu from './Components/Menu';

import { createTheme, ThemeProvider } from '@mui/material/styles';
import { PaletteMode, ThemeOptions } from '@mui/material';
import { Outlet } from 'react-router-dom';

const ThemeContext = React.createContext({ toggleTheme: () => {} });

const getDesignTokens = (mode: PaletteMode) => ({
  palette: {
    mode
  },
});

function App() {
  const [mode, setMode] = React.useState<PaletteMode>('dark');

  const toggleTheme = {
    toggleTheme: () => {
      const nextTheme = mode === 'light'
        ? 'dark'
        : 'light';

      setMode(nextTheme);
    },
  }

  const theme = React.useMemo(
    () => createTheme(getDesignTokens(mode)),
    [mode]
  );

  return (
    <ThemeContext.Provider value={toggleTheme}>
      <ThemeProvider theme={theme}>
        <div className="App">
          <nav data-tauri-drag-region="true">
            <Menu />
            <div className='navbar-center'>
              <p className='navbar-center-text'>File Share</p>
            </div>
            <div className='navbar-right'>
              <button className='window-button' onClick={() => appWindow.minimize()}>
                <RemoveRoundedIcon className='window-button-icon' />
              </button>
              <button className='window-button' onClick={() => appWindow.toggleMaximize()}>
                <CropSquareRounded className='window-button-icon' />
              </button>
              <button className='window-button window-exit' onClick={() => appWindow.close()}>
                <CloseRoundedIcon className='window-button-icon' />
              </button>
            </div>
          </nav>
          <div className='window-content'>
            <Outlet />
          </div>
        </div>
      </ThemeProvider>
    </ThemeContext.Provider>
  );
}

export { App, ThemeContext };
