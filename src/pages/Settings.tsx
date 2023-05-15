import {
  Box,
  Button,
  Container,
  Divider,
  FormGroup,
  Paper,
  Stack,
  Switch,
  Typography,
} from "@mui/material";
import { invoke } from "@tauri-apps/api";
import { open } from "@tauri-apps/api/dialog";
import React from "react";
import { Settings, SettingsContext, ThemeContext } from "../App";


function SettingsPage() {
  const { mode, toggleTheme } = React.useContext(ThemeContext);
  const { settings, updateSettings } = React.useContext(SettingsContext);

  const handleSave = async () => {
    const newSettings = {
      ...settings,
      theme: mode,
    };

    updateSettings(newSettings);

    await invoke("save_settings", { message: newSettings });
  };

  const handleSaveDirectoryChange = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });

    if (typeof selected == "string") {
      const newSettings: Settings = {
        ...settings,
        downloadDirectory: selected,
      };

      updateSettings(newSettings);
    }
  };

  const handleShowCurrentDirectory = async () => {
    if (settings.downloadDirectory) {
      await invoke("open_file", {
        message: {
          file_path: settings.downloadDirectory,
        },
      });
    }
  };

  const handleChangeMinimize = async () => {
    const newMinimizeOption = !settings.minimizeOnClose;

    const newSettings: Settings = {
      ...settings,
      minimizeOnClose: newMinimizeOption,
    };

    updateSettings(newSettings);
  };

  return (
    <Container>
      <Paper elevation={2}>
        {settings && (
          <Box sx={{ padding: "2em" }}>
            <Typography variant="h3" marginBottom={"0.75em"}>
              Settings
            </Typography>
            <Stack
              direction="row"
              sx={{ marginBottom: "1em" }}
              spacing={2}
              divider={<Divider orientation="vertical" flexItem />}
            >
              <Stack spacing={1}>
                <FormGroup>
                  <Typography>Minimize On Close</Typography>
                  <Switch
                    checked={settings.minimizeOnClose}
                    onChange={handleChangeMinimize}
                  />
                </FormGroup>
                <FormGroup>
                  <Typography>Theme</Typography>
                  <Switch
                    checked={mode === "dark"}
                    onChange={() => toggleTheme()}
                  />
                </FormGroup>
              </Stack>
              <Stack>
                <FormGroup>
                  <Typography>Save Directory</Typography>
                  <Button
                    variant="contained"
                    style={{ margin: "0.5em 0em" }}
                    onClick={handleShowCurrentDirectory}
                  >
                    Show Folder
                  </Button>
                  <Button
                    variant="contained"
                    style={{ margin: "0.5em 0em" }}
                    onClick={handleSaveDirectoryChange}
                  >
                    Set Directory
                  </Button>
                </FormGroup>
              </Stack>
            </Stack>
            <Button color="success" onClick={handleSave} variant="contained">
              Save
            </Button>
          </Box>
        )}
      </Paper>
    </Container>
  );
}

export default SettingsPage;
