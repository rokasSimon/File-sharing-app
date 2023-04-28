import React from "react";
import { listen } from "@tauri-apps/api/event";
import ClearIcon from "@mui/icons-material/Clear";
import {
  Box,
  IconButton,
  LinearProgress,
  Paper,
  Stack,
  Typography,
} from "@mui/material";
import { CancelDownload, invokeNetworkCommand } from "./networkCommands";

type DownloadingFile = {
  downloadId: string;
  directoryIdentifier: string;
  fileIdentifier: string;
  fileName: string;
  progress: number;
  canceled: boolean;
};

const initialState: Array<DownloadingFile> = [];
// const DownloadsContext =
//   React.createContext<Array<DownloadingFile>>(initialState);

function DownloadsManager({ children }: any) {
  const [downloads, setDownloads] = React.useState(initialState);
  const downloadsRef = React.useRef(downloads);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    downloadsRef.current = downloads;
  }, [downloads]);

  React.useEffect(() => {
    if (loaded.current) return;

    const startListenDownloads = async () => {
      const _ = await listen<DownloadingFile>("DownloadingFile", (event) => {
        const input = event.payload;

        const alreadyDownloading = downloadsRef.current.find((download) => {
          return download.downloadId === input.downloadId;
        });

        let updatedDownloads = [];
        if (alreadyDownloading) {
          if (input.progress === 100) {
            updatedDownloads = downloadsRef.current.filter((download) => {
              return download.downloadId !== input.downloadId;
            });
          } else {
            alreadyDownloading.progress = input.progress;

            updatedDownloads = downloadsRef.current;
          }
        } else {
          updatedDownloads = [input, ...downloadsRef.current];
        }

        setDownloads(updatedDownloads);
      });
    };

    const startListenDownloadCanceled = async () => {
      const _ = await listen<DownloadingFile>("CanceledDownload", (event) => {
        const input = event.payload;

        // const currentDownloads = [input, ...downloadsRef.current];

        // setDownloads(currentDownloads);
      });
    };

    startListenDownloads();
    startListenDownloadCanceled();

    loaded.current = true;
  }, []);

  const handleDownloadCancel = (downloadId: string) => async () => {
    const download = downloads.find((d) => d.downloadId === downloadId);

    if (download) {
      const request: CancelDownload = {
        cancelDownload: {
          download_identifier: downloadId
        }
      };

      await invokeNetworkCommand(request);
    }
  };

  const downloadIndicators = downloads.map((download) => {
    return (
      <Paper
        elevation={2}
        variant="outlined"
        style={{
          padding: "0.5em 1em",
        }}
      >
        <Box display={"flex"} justifyContent={"space-between"}>
          <Typography variant="caption">{download.fileName}</Typography>
          <IconButton size="small" onClick={handleDownloadCancel(download.downloadId)}>
            <ClearIcon fontSize="small" />
          </IconButton>
        </Box>
        <LinearProgress variant="determinate" value={download.progress} />
      </Paper>
    );
  });

  return (
    <React.Fragment>
      {children}
      <Box
        position={"absolute"}
        minWidth={"10vw"}
        marginBottom={"2em"}
        marginRight={"2em"}
        bottom={0}
        right={0}
      >
        <Stack spacing={1}>{downloadIndicators}</Stack>
      </Box>
    </React.Fragment>
  );
}

export { DownloadsManager };
