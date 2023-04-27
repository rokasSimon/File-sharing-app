import {
  Box,
  Breadcrumbs,
  Button,
  IconButton,
  Link,
  Paper,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableFooter,
  TableHead,
  TableRow,
} from "@mui/material";
import InfoRoundedIcon from "@mui/icons-material/InfoRounded";
import NavigateNextRoundedIcon from "@mui/icons-material/NavigateNextRounded";
import FolderIcon from "@mui/icons-material/Folder";
import { SharedFile } from "../RustCommands/ShareDirectoryContext";

import { open } from "@tauri-apps/api/dialog";
import {
  AddFiles,
  invokeNetworkCommand,
} from "../RustCommands/networkCommands";
import React from "react";

type DirectoryDetailsProps = {
  files: Map<string, SharedFile>;
  directoryName: string;
  directoryIdentifier: string;
};

function toLargestDenominator(size: number): string {
  const sizes = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
  const k = 1024;
  const decimals = 1;

  const i = Math.floor(Math.log(size) / Math.log(k));

  return `${parseFloat((size / Math.pow(k, i)).toFixed(decimals))} ${sizes[i]}`;
}

function DirectoryDetails({
  files,
  directoryName,
  directoryIdentifier,
}: DirectoryDetailsProps) {
  const handleAddFiles = async () => {
    const selected = await open({
      multiple: true,
    });

    if (selected) {
      const files: string[] = Array.isArray(selected) ? selected : [selected];

      const request: AddFiles = {
        addFiles: {
          file_paths: files,
          directory_identifier: directoryIdentifier,
        },
      };

      await invokeNetworkCommand(request);
    }
  };

  let rows = [];
  for (const [id, file] of files.entries()) {
    const row = (
      <TableRow key={id}>
        <TableCell variant="body">{file.name}</TableCell>
        <TableCell variant="body" align="right">
          {toLargestDenominator(file.size)}
        </TableCell>
        <TableCell variant="body" align="right">
          {new Date(file.lastModified).toDateString()}
        </TableCell>
        <TableCell variant="body" align="right">
          <IconButton>
            <InfoRoundedIcon />
          </IconButton>
        </TableCell>
      </TableRow>
    );

    rows.push(row);
  }

  if (rows.length < 1) {
    const emptyFileRow = (
      <TableRow>
        <TableCell align="center" colSpan={4}>No files found in this directory</TableCell>
      </TableRow>
    );

    rows.push(emptyFileRow);
  }

  return (
    <Box sx={{ height: "100%", margin: "1em", marginRight: "2.5em" }}>
      <Box
        display={"flex"}
        justifyContent={"space-between"}
        marginTop={"1em"}
        marginBottom={"1em"}
      >
        <Breadcrumbs
          style={{
            margin: "0.25em 0.25em 1em 0.25em",
            padding: "0.5em",
            alignItems: "center",
          }}
          separator={<NavigateNextRoundedIcon fontSize="small" />}
        >
          <FolderIcon />
          <Link underline="hover" color="info">
            {directoryName}
          </Link>
        </Breadcrumbs>
        <Button variant="contained" onClick={handleAddFiles} size="small">
          Add Files
        </Button>
      </Box>
      <TableContainer component={Paper} elevation={2} variant="elevation">
        <Table
          style={{
            height: "100%",
          }}
        >
          <TableHead>
            <TableRow>
              <TableCell variant="head">File Name</TableCell>
              <TableCell variant="head" align="right">
                Size
              </TableCell>
              <TableCell variant="head" align="right">
                Last Modified
              </TableCell>
              <TableCell variant="head" align="right">
                Details
              </TableCell>
            </TableRow>
          </TableHead>
          <TableBody>{rows}</TableBody>
          <TableFooter>
            <TableRow>
              <TableCell colSpan={4}></TableCell>
            </TableRow>
          </TableFooter>
        </Table>
      </TableContainer>
    </Box>
  );
}

export default DirectoryDetails;