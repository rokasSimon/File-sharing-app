import React from "react";
import "./App.css";

import RemoveRoundedIcon from "@mui/icons-material/RemoveRounded";
import CloseRoundedIcon from "@mui/icons-material/CloseRounded";
import CropSquareRounded from "@mui/icons-material/CropSquareRounded";

import { appWindow } from "@tauri-apps/api/window";
import Menu from "./Components/Menu";

import { createTheme, ThemeProvider } from "@mui/material/styles";
import { PaletteMode, Paper } from "@mui/material";
import { Outlet, useNavigate } from "react-router-dom";
import { ShareDirectoryProvider } from "./RustCommands/ShareDirectoryContext";
import { ConnectedDevicesProvider } from "./RustCommands/ConnectedDevicesContext";
import { DownloadsManager } from "./RustCommands/DownloadsManager";
import { listen } from "@tauri-apps/api/event";
import { message } from "@tauri-apps/api/dialog";
import { invoke } from "@tauri-apps/api";

type BackendError = {
  title: string;
  error: string;
};

type ThemeContextValue = {
  toggleTheme: () => void;
  mode: "light" | "dark";
};

type Settings = {
  minimizeOnClose: boolean;
  theme: "light" | "dark";
  downloadDirectory: string;
};

const initialSettings: Settings = {
  minimizeOnClose: false,
  theme: "dark",
  downloadDirectory: "",
};
const SettingsContext = React.createContext({ updateSettings: (settings: Settings) => {}, settings: initialSettings });
const ThemeContext = React.createContext<ThemeContextValue>({
  toggleTheme: () => {},
  mode: "dark",
});
const ErrorContext = React.createContext<BackendError | null>(null);

const getDesignTokens = (mode: PaletteMode) => ({
  palette: {
    mode,
  },
});

function App() {
  const navigate = useNavigate();
  const [lastError, setLastError] = React.useState<BackendError | null>(null);
  const [mode, setMode] = React.useState<PaletteMode>("dark");
  const [settings, setSettings] = React.useState<Settings>(initialSettings);

  const toggleTheme = {
    toggleTheme: () => {
      const nextTheme = mode === "light" ? "dark" : "light";

      setMode(nextTheme);
    },
  };

  const theme = React.useMemo(() => createTheme(getDesignTokens(mode)), [mode]);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    if (loaded.current) return;

    const startListenErrors = async () => {
      const _ = await listen<BackendError>("Error", async (event) => {
        const input = event.payload;

        setLastError(input);

        await message(input.error, { title: input.title, type: "error" });
      });
    };

    const getSettings = async () => {
      const loadedSettings = await invoke<Settings | string>("get_settings", {
        message: "",
      });

      if (typeof loadedSettings == "string") {
        console.error(loadedSettings);
      } else {
        if (loadedSettings.theme !== mode) {
          toggleTheme.toggleTheme();
        }

        setSettings(loadedSettings);
      }
    };

    getSettings();

    startListenErrors();
    navigate("/directories");

    loaded.current = true;
  }, []);

  const handleClose = () => {
    if (settings.minimizeOnClose) {
      appWindow.hide();
    } else {
      appWindow.close();
    }
  }

  const themeVal: ThemeContextValue = {
    toggleTheme: toggleTheme.toggleTheme,
    mode: mode,
  };

  return (
    <ThemeContext.Provider value={themeVal}>
      <ThemeProvider theme={theme}>
        <SettingsContext.Provider value={{
          settings: settings,
          updateSettings: setSettings
        }}>
          <ErrorContext.Provider value={lastError}>
            <ConnectedDevicesProvider>
              <ShareDirectoryProvider>
                <div className="App">
                  <div className="window-nav" data-tauri-drag-region="true">
                    <Menu />
                    <div className="navbar-center">
                      <p className="navbar-center-text">File Share</p>
                    </div>
                    <div className="navbar-right">
                      <button
                        className="window-button"
                        onClick={() => appWindow.minimize()}
                      >
                        <RemoveRoundedIcon className="window-button-icon" />
                      </button>
                      <button
                        className="window-button"
                        onClick={() => appWindow.toggleMaximize()}
                      >
                        <CropSquareRounded className="window-button-icon" />
                      </button>
                      <button
                        className="window-button window-exit"
                        onClick={handleClose}
                      >
                        <CloseRoundedIcon className="window-button-icon" />
                      </button>
                    </div>
                  </div>
                  <Paper
                    style={{
                      flex: "1 1 auto",
                      padding: "1rem",
                      borderRadius: 0,
                    }}
                  >
                    <DownloadsManager>
                      <Outlet />
                    </DownloadsManager>
                  </Paper>
                </div>
              </ShareDirectoryProvider>
            </ConnectedDevicesProvider>
          </ErrorContext.Provider>
        </SettingsContext.Provider>
      </ThemeProvider>
    </ThemeContext.Provider>
  );
}

export { App, ThemeContext, ErrorContext, SettingsContext };
export type { Settings };
