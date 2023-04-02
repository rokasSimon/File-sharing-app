import React from 'react';

import './Menu.css';
import { ThemeContext } from '../App';
import { useNavigate } from 'react-router-dom';

function Menu() {
    const themeContext = React.useContext(ThemeContext);
    const navigate = useNavigate();

    return (
      <div className='navbar-left'>
          <button className="menu-button" onClick={() => navigate('/directories')}>
            Dashboard
          </button>
          <button className="menu-button" onClick={() => navigate('/settings')}>
            Settings
          </button>
          <button className="menu-button" onClick={() => themeContext.toggleTheme()}>
            Toggle
          </button>
      </div>
    );
  }
  
  export default Menu;
  