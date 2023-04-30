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

type Download = {
  downloadId: string;
  fileIdentifier: string;
  directoryIdentifier: string;
  progress: number;
  fileName: string;
  filePath: string;
  canceled: boolean;
};

type DownloadUpdate = {
  progress: number;
  downloadId: string;
};

type DownloadCanceled = {
  reason: string;
  downloadId: string;
};

function DownloadsManager({ children }: any) {
  const [downloads, setDownloads] = React.useState<Download[]>([]);
  const downloadsRef = React.useRef(downloads);
  const loaded = React.useRef(false);

  React.useEffect(() => {
    downloadsRef.current = downloads;
  }, [downloads]);

  React.useEffect(() => {
    if (loaded.current) return;

    const startListenDownloadStart = async () => {
      const _ = await listen<Download>("DownloadStarted", (event) => {
        const input = event.payload;
        console.log(`Download started ${JSON.stringify(input)}`);

        const updatedDownloads = [
          input,
          ...downloadsRef.current
        ];

        setDownloads(updatedDownloads);
      });
    };

    const startListenDownloadUpdate = async () => {
      const _ = await listen<DownloadUpdate>("DownloadUpdate", (event) => {
        const input = event.payload;
        console.log(`Download updated ${JSON.stringify(input)}`);

        const alreadyDownloading = downloadsRef.current.find((download) => {
          return download.downloadId === input.downloadId;
        });

        if (alreadyDownloading) {
          alreadyDownloading.progress = input.progress;

          if (alreadyDownloading.progress === 100) {
            setTimeout(() => {
              const downloadsToKeep = downloadsRef.current.filter((download) => {
                return download.downloadId !== alreadyDownloading?.downloadId;
              });
    
              setDownloads(downloadsToKeep);
            }, 5000);
          }
        }

        setDownloads([ ...downloadsRef.current ]);
      });
    };

    const startListenDownloadCanceled = async () => {
      const _ = await listen<DownloadCanceled>("DownloadCanceled", (event) => {
        const input = event.payload;

        const alreadyDownloading = downloadsRef.current.find((download) => {
          return download.downloadId === input.downloadId;
        });

        if (alreadyDownloading) {
          alreadyDownloading.canceled = true;
        }

        setDownloads([ ...downloadsRef.current ]);

        setTimeout(() => {
          const downloadsToKeep = downloadsRef.current.filter((download) => {
            return download.downloadId !== alreadyDownloading?.downloadId;
          });

          setDownloads(downloadsToKeep);
        }, 5000);
      });
    };

    startListenDownloadStart();
    startListenDownloadUpdate();
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
    const startColor = download.progress === 100 ? "success" : "primary";
    const color = download.canceled ? "error" : startColor;

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
        <LinearProgress variant="determinate" value={download.progress} color={color} />
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
