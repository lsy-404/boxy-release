const form = document.getElementById('service-form');
const input = document.getElementById('service-url');
const error = document.getElementById('error');

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  input.disabled = true;
  error.textContent = '';
  try {
    await window.__TAURI__.core.invoke('select_service', { serverUrl: input.value.trim() });
  } catch (reason) {
    error.textContent = String(reason);
    input.disabled = false;
    input.focus();
  }
});

const connectStatus = document.getElementById('connect-status');
document.getElementById('connect-config').addEventListener('click', async () => {
  try { if (await window.__TAURI__.core.invoke('configure_connect')) connectStatus.textContent = '已选择配置'; }
  catch (reason) { connectStatus.textContent = String(reason); }
});
document.getElementById('panel-install').addEventListener('click', async () => {
  try { if (await window.__TAURI__.core.invoke('install_connect_panel')) connectStatus.textContent = '已保存侧栏脚本，请在 Synthesizer V 中重新扫描脚本。'; }
  catch (reason) { connectStatus.textContent = String(reason); }
});
