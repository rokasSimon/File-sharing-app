import {
  Box,
  Button,
  Container,
  FormControlLabel,
  FormGroup,
  Paper,
  Stack,
  Switch,
  TextField,
  Typography,
} from "@mui/material";
import { invoke } from "@tauri-apps/api";
import React from "react";
import { ThemeContext } from "../App";

type Settings = {
  minimizeOnClose: boolean;
  theme: "light" | "dark";
  downloadDirectory: string;
};

function Settings() {
  const theme = React.useContext(ThemeContext);
  const [settings, setSettings] = React.useState<Settings | null>(null);
  const settingsRef = React.useRef(settings);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  React.useEffect(() => {
    if (loaded.current) return;

    const getSettings = async () => {
      const loadedSettings = await invoke<Settings | string>("get_settings", {
        message: "",
      });

      if (typeof loadedSettings == "string") {
        console.error(loadedSettings);
      } else {
        setSettings(loadedSettings);
      }
    };

    getSettings();

    loaded.current = true;
  }, []);

  const handleSave = async () => {
    if (settings) {
      await invoke("save_settings", { message: settings });
    }
  };

  return (
    <Container>
      <Paper elevation={2}>
        {settings && (
          <Box sx={{ padding: "2em" }}>
            <Typography variant="h3" marginBottom={"0.75em"}>
              Settings
            </Typography>
            <Stack direction="row" sx={{ marginBottom: '1em' }}>
              <Stack spacing={1}>
                <FormGroup>
                  <Typography>Minimize On Close</Typography>
                  <Switch checked={settings.minimizeOnClose} />
                </FormGroup>
                <FormGroup>
                  <Typography>Theme</Typography>
                  <Switch
                    checked={theme.mode === "dark"}
                    onChange={() => theme.toggleTheme()}
                  />
                </FormGroup>
              </Stack>
              <Stack>
                <FormGroup>
                    <TextField value={settings.downloadDirectory} disabled />
                </FormGroup>
              </Stack>
            </Stack>
            <Button color="success" onClick={handleSave}>
              Save
            </Button>
          </Box>
        )}
      </Paper>
    </Container>
  );
}

export default Settings;
