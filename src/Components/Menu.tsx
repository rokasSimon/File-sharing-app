import React from "react";

import "./Menu.css";
import { ThemeContext } from "../App";
import { useNavigate } from "react-router-dom";
import { Button, Menu as MaterialMenu, MenuItem } from "@mui/material";

function Menu() {
  const themeContext = React.useContext(ThemeContext);
  const navigate = useNavigate();

  const [anchorEl, setAnchorEl] = React.useState<null | HTMLElement>(null);
  const open = Boolean(anchorEl);
  const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
    setAnchorEl(event.currentTarget);
  };
  const handleClose = () => {
    setAnchorEl(null);
  };

  return (
    <div className="navbar-left">
      <Button id="menu-button" color="info" onClick={handleClick}>
        Menu
      </Button>
      <MaterialMenu
        id="basic-menu"
        anchorEl={anchorEl}
        open={open}
        onClose={handleClose}
      >
        <MenuItem
          onClick={() => {
            handleClose();
            navigate("/directories");
          }}
        >
          Dashboard
        </MenuItem>
        <MenuItem
          onClick={() => {
            handleClose();
            navigate("/settings");
          }}
        >
          Settings
        </MenuItem>
        <MenuItem
          onClick={() => {
            handleClose();
            themeContext.toggleTheme();
          }}
        >
          Toggle Theme
        </MenuItem>
      </MaterialMenu>
    </div>
  );
}

export default Menu;
