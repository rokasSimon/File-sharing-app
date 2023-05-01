import React from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';
import { App } from './App';
import reportWebVitals from './reportWebVitals';

import { createHashRouter, RouterProvider } from "react-router-dom";

import '@fontsource/roboto/500.css';
import Directories from './pages/Directories';
import SettingsPage from './pages/Settings';

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);

const router = createHashRouter([
  {
    path: '/',
    element: <App />,
    children: [
      {
        path: '/settings',
        element: <SettingsPage />
      },
      {
        path: '/directories',
        element: <Directories />
      }
    ]
  }
]);

root.render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>
);

// If you want to start measuring performance in your app, pass a function
// to log results (for example: reportWebVitals(console.log))
// or send to an analytics endpoint. Learn more: https://bit.ly/CRA-vitals
reportWebVitals();
