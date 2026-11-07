window.__TAURI__.event.listen('fps', (e) => {
  document.getElementById('fps').textContent = Math.round(e.payload);
});
