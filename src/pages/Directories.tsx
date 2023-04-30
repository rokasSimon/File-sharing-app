import "./Directories.css";

import MoreHorizIcon from "@mui/icons-material/MoreHoriz";
import ResizableBox from "../Components/ResizableBox";
import React from "react";
import Dialog from "@mui/material/Dialog";
import {
  Menu as MaterialMenu,
  Box,
  Button,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  ListSubheader,
  Paper,
  TextField,
  Typography,
  MenuItem,
  DialogContentText,
  Checkbox,
  ListItemIcon,
} from "@mui/material";
import {
  CreateShareDirectory,
  LeaveDirectory,
  ShareDirectoryToPeers,
  invokeNetworkCommand,
} from "../RustCommands/networkCommands";
import {
  PeerId,
  ShareDirectory,
  ShareDirectoryContext,
  SharedFile,
} from "../RustCommands/ShareDirectoryContext";
import DirectoryDetails from "../Components/DirectoryDetails";
import {
  ConnectedDevicesContext,
  GetPeers,
} from "../RustCommands/ConnectedDevicesContext";

type SharePeer = {
  peer: PeerId;
  sharedBefore: boolean;
  checked: boolean;
};

function Directories() {
  const shareDirectories = React.useContext(ShareDirectoryContext);
  const [selectedDirectory, setSelectedDirectory] =
    React.useState<ShareDirectory | null>(null);
  const [shareCreationName, setShareCreationName] = React.useState("");
  const [shareCreationOpen, setShareCreationOpen] = React.useState(false);

  React.useEffect(() => {
    console.log("Updating directories from context" + JSON.stringify(shareDirectories));
    if (selectedDirectory) {
      const directory = shareDirectories.find((dir) => dir.signature.identifier === selectedDirectory.signature.identifier);

      if (directory) {
        console.log(`Setting ${JSON.stringify(selectedDirectory)} to ${JSON.stringify(directory)}`);
        setSelectedDirectory(directory);
      } else {
        setSelectedDirectory(null);
      }
    }
  }, [shareDirectories]);

  const [optDirectory, setOptDirectory] = React.useState<ShareDirectory | null>(
    null
  );
  const [optAnchorEl, setOptAnchorEl] = React.useState<null | HTMLElement>(
    null
  );
  const optOpen = Boolean(optAnchorEl);
  const listItemClickable = !optOpen;

  const peers = React.useContext(ConnectedDevicesContext);
  const [sharePeers, setSharePeers] = React.useState<SharePeer[] | null>(null);

  React.useEffect(() => {
    if (optDirectory && peers.length > 0) {
      const sp = peers.map((peer) => {
        const matchedPeer = optDirectory.signature.sharedPeers.find(
          (p) => p.uuid === peer.uuid
        );
        const result: SharePeer = {
          peer,
          sharedBefore: matchedPeer != undefined,
          checked: matchedPeer != undefined,
        };
  
        return result;
      });

      setSharePeers(sp);
    } else {
      setSharePeers(null);
    }
  }, [peers, optDirectory]);

  const [shareOpen, setShareOpen] = React.useState(false);
  const [leaveOpen, setLeaveOpen] = React.useState(false);

  const handleOpenCreate = () => {
    setShareCreationName("");
    setShareCreationOpen(true);
  };
  const handleCloseCreate = () => {
    setShareCreationName("");
    setShareCreationOpen(false);
  };

  const handleNameChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setShareCreationName(event.currentTarget.value);
  };

  const handleCreate = async () => {
    let request: CreateShareDirectory = {
      createShareDirectory: shareCreationName,
    };

    await invokeNetworkCommand(request);

    handleCloseCreate();
  };

  const handleListClick = async (identifier: string) => {
    if (listItemClickable) {
      const directory = shareDirectories.find((dir) => {
        return dir.signature.identifier === identifier;
      });

      if (directory) {
        setSelectedDirectory({ ...directory });
      } else {
        setSelectedDirectory(null);
      }
    }
  };

  const handleDirectoryOptionsOpen = (
    event: React.MouseEvent<HTMLButtonElement>,
    identifier: string
  ) => {
    const directory = shareDirectories.find((dir) => {
      return dir.signature.identifier === identifier;
    });

    if (directory) {
      setOptDirectory({ ...directory });
      setOptAnchorEl(event.currentTarget);
    } else {
      setOptDirectory(null);
    }
  };

  const handleDirectoryOptionsClose = () => {
    setOptDirectory(null);
    setOptAnchorEl(null);
  };

  const handleShareToggle = (value: string) => () => {
    if (sharePeers) {
      const newCheckedPeers = [...sharePeers];

      for (const p of newCheckedPeers) {
        if (p && p.peer.uuid === value && p.sharedBefore != true) {
          p.checked = !p.checked;
        }
      }

      setSharePeers(newCheckedPeers);
    }
  };

  let peerList: JSX.Element | JSX.Element[] = [];
  if (sharePeers) {
    peerList = sharePeers.map((p) => {
      return (
        <ListItem key={p.peer.uuid}>
          <ListItemButton onClick={handleShareToggle(p.peer.uuid)}>
            <ListItemIcon>
              <Checkbox
                edge="end"
                checked={p.checked}
                readOnly={p.sharedBefore}
              />
            </ListItemIcon>
            <ListItemText primary={p.peer.hostname} />
          </ListItemButton>
        </ListItem>
      );
    });
  }

  if (peerList.length == 0) {
    peerList = (
      <ListItem>
        <ListItemText>No peers connected</ListItemText>
      </ListItem>
    );
  }

  const handleShareOpen = (directoryIdentifier: string | undefined) => {
    if (directoryIdentifier) {
      handleDirectoryOptionsClose();

      const dir = shareDirectories.find(
        (d) => d.signature.identifier === directoryIdentifier
      );
      if (!dir) return;

      const request: GetPeers = {
        getPeers: true,
      };

      invokeNetworkCommand(request).then(() => {
        setOptDirectory(dir);
        setShareOpen(true);
      });
    }
  };

  const handleShareClose = () => {
    setOptDirectory(null);
    setShareOpen(false);
  };

  const handleShare = async () => {
    if (!optDirectory || !sharePeers) return;

    const peersToShareTo = sharePeers
      .filter((peer) => {
        return peer.checked && !peer.sharedBefore;
      })
      .map((peer) => peer.peer);

    const request: ShareDirectoryToPeers = {
      shareDirectoryToPeers: {
        directory_identifier: optDirectory.signature.identifier,
        peers: peersToShareTo,
      },
    };

    await invokeNetworkCommand(request);

    handleShareClose();
  };

  const handleLeaveOpen = (directoryIdentifier: string | undefined) => {
    if (directoryIdentifier) {
      handleDirectoryOptionsClose();

      const dir = shareDirectories.find(
        (d) => d.signature.identifier === directoryIdentifier
      );
      if (!dir) return;

      setOptDirectory(dir);
      setLeaveOpen(true);
    }
  };

  const handleLeaveClose = () => {
    setOptDirectory(null);
    setLeaveOpen(false);
  };

  const handleLeave = async () => {
    if (selectedDirectory?.signature.identifier) {
      const request: LeaveDirectory = {
        leaveDirectory: {
          directory_identifier: selectedDirectory.signature.identifier
        }
      };

      await invokeNetworkCommand(request);
    }
  };

  const duplicatedNames = new Map<string, number>();
  let directories = null;
  if (shareDirectories) {
    directories = shareDirectories.map((val, i) => {
      const usedCount = duplicatedNames.get(val.signature.name);

      if (usedCount === undefined) {
        duplicatedNames.set(val.signature.name, 1);
      } else {
        duplicatedNames.set(val.signature.name, usedCount + 1);
      }

      return (
        <ListItem
          key={val.signature.identifier}
          divider={true}
          secondaryAction={
            <IconButton
              edge="end"
              onClick={(event) =>
                handleDirectoryOptionsOpen(event, val.signature.identifier)
              }
            >
              <MoreHorizIcon />
            </IconButton>
          }
        >
          <ListItemButton
            style={{ maxHeight: "3em" }}
            onClick={() => handleListClick(val.signature.identifier)}
          >
            <ListItemText>
              {val.signature.name}
              {usedCount && (
                <Typography
                  variant="caption"
                  color="GrayText"
                >{` (${usedCount})`}</Typography>
              )}
            </ListItemText>
          </ListItemButton>
        </ListItem>
      );
    });
  }

  const leftContainer = (
    <div id="directory-column">
      <div id="start-button-box">
        <Button
          onClick={handleOpenCreate}
          variant="contained"
          id="start-share-directory-btn"
        >
          Start Share Directory
        </Button>
        <MaterialMenu
          anchorEl={optAnchorEl}
          open={optOpen}
          onClose={handleDirectoryOptionsClose}
        >
          <MenuItem
            onClick={() =>
              handleLeaveOpen(optDirectory?.signature?.identifier)
            }
          >
            Remove
          </MenuItem>
          <MenuItem
            onClick={() => handleShareOpen(optDirectory?.signature?.identifier)}
          >
            Share
          </MenuItem>
        </MaterialMenu>
      </div>
      <List id="directories">{directories}</List>
    </div>
  );

  const rightContainer = selectedDirectory ? (
    <DirectoryDetails
      files={selectedDirectory.shared_files}
      directoryName={selectedDirectory.signature.name}
      directoryIdentifier={selectedDirectory.signature.identifier}
      currentPeers={peers}
    />
  ) : null;

  return (
    <div id="directories-page">
      <div id="directories-view">
        <ResizableBox
          leftContainer={leftContainer}
          rightContainer={rightContainer}
          minWidth={"10vw"}
          maxWidth={"80vw"}
        />
      </div>

      <Dialog open={shareCreationOpen} onClose={handleCloseCreate}>
        <div>
          <DialogTitle>Creating New Share Directory</DialogTitle>
          <DialogContent>
            <TextField
              id="share-directory-name"
              label="Directory Name"
              variant="standard"
              value={shareCreationName}
              onChange={handleNameChange}
            />
          </DialogContent>
          <DialogActions>
            <Button onClick={handleCloseCreate}>Cancel</Button>
            <Button onClick={handleCreate}>Create</Button>
          </DialogActions>
        </div>
      </Dialog>

      <Dialog open={shareOpen} onClose={handleShareClose}>
        <div>
          <DialogTitle>Directory Sharing</DialogTitle>
          <DialogContent>
            <DialogContentText>
              Select connected devices to reveal directory to.
            </DialogContentText>
            <List>{peerList}</List>
          </DialogContent>
          <DialogActions>
            <Button onClick={handleShareClose}>Cancel</Button>
            <Button onClick={handleShare}>Share</Button>
          </DialogActions>
        </div>
      </Dialog>

      <Dialog open={leaveOpen} onClose={handleLeaveClose}>
        <div>
          <DialogTitle>Leave Directory</DialogTitle>
          <DialogContent>
            <DialogContentText>
              Are you sure you want to leave directory {selectedDirectory?.signature.name}?
            </DialogContentText>
          </DialogContent>
          <DialogActions>
            <Button onClick={handleLeaveClose}>Cancel</Button>
            <Button onClick={handleLeave}>Leave</Button>
          </DialogActions>
        </div>
      </Dialog>
    </div>
  );
}

export default Directories;
