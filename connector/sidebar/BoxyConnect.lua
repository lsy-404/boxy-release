local host = SV:getHostInfo()
local separator = host.osType == "Windows" and "\\" or "/"
local base = os.getenv("BOXY_PANEL_DIR")
if not base or base == "" then
    local home = host.osType == "Windows" and os.getenv("LOCALAPPDATA") or os.getenv("HOME")
    base = (home or ".") .. separator .. "Boxy" .. separator .. "ConnectPanel"
end
local statusPath = base .. separator .. "status.txt"
local requestPath = base .. separator .. "request.txt"
local values = {}
for _, name in ipairs({"status", "selection", "process", "conversation", "input", "source", "main", "send"}) do
    values[name] = SV:create("WidgetValue")
end
values.status:setEnabled(false)
values.conversation:setEnabled(false)
values.source:setEnabled(false)
values.main:setEnabled(false)
values.send:setEnabled(false)
values.selection:setValue(0)
values.process:setValue(0)
values.input:setValue("")
values.conversation:setValue("")
local current = {live=false, account=false, chat=false}
local lastResponse = ""
local sourceIds, sourceNames, processIds, processNames = {}, {"无可用来源"}, {}, {"无可用实例"}
local lastSources, lastProcesses = "", ""

local function readFile(path)
    local f = io.open(path, "rb")
    if not f then return nil end
    local data = f:read(65537)
    f:close()
    if not data or #data > 65536 then return nil end
    return data
end

local function fields(data)
    local result = {}
    for line in (data or ""):gmatch("[^\r\n]+") do
        local key, value = line:match("^([%w_]+)=(.*)$")
        if key then result[key] = value end
    end
    return result
end

local function decode(value)
    if not value or #value % 2 ~= 0 or value:find("[^%da-fA-F]") then return "" end
    return (value:gsub("..", function(pair) return string.char(tonumber(pair, 16)) end))
end

local function encode(value)
    return (value:gsub(".", function(byte) return string.format("%02x", string.byte(byte)) end))
end

local function writeRequest(kind, payload)
    if not current.live or readFile(requestPath) then return false end
    local temp = requestPath .. ".tmp"
    local file = io.open(temp, "wb")
    if not file then return false end
    local ok = file:write("version=1\nkind=", kind, "\npayload_hex=", encode(payload), "\n")
    file:flush()
    file:close()
    if not ok or not os.rename(temp, requestPath) then os.remove(temp); return false end
    return true
end

values.source:setValueChangeCallback(function()
    local selection = sourceIds[(tonumber(values.selection:getValue()) or -1) + 1]
    local pid = processIds[(tonumber(values.process:getValue()) or -1) + 1]
    if current.account and selection and pid then
        writeRequest("account.source.select", pid .. "\t" .. selection)
    end
end)
values.main:setValueChangeCallback(function()
    local pid = processIds[(tonumber(values.process:getValue()) or -1) + 1]
    if current.account and pid then writeRequest("account.main.restore", pid) end
end)
values.send:setValueChangeCallback(function()
    local message = tostring(values.input:getValue() or "")
    if current.chat and #message > 0 and #message <= 8192 and writeRequest("chat.send", message) then
        values.input:setValue("")
    end
end)

local function refresh()
    local state = fields(readFile(statusPath))
    local timestamp = tonumber(state.updated_at or "")
    current.live = timestamp ~= nil and math.abs(os.time() - timestamp) <= 5
    current.account = current.live and state.boxy_authorized == "1" and state.account_available == "1"
    current.chat = current.live and state.boxy_authorized == "1" and state.chat_available == "1"
    local sources = decode(state.sources_hex)
    local processes = decode(state.processes_hex)
    if sources ~= lastSources or processes ~= lastProcesses then
        if sources ~= lastSources then
            sourceIds, sourceNames = {}, {}
            for line in sources:gmatch("[^\n]+") do
                local id, label = line:match("^([^\t]+)\t(.*)$")
                if id then sourceIds[#sourceIds+1] = id; sourceNames[#sourceNames+1] = label end
            end
            if #sourceNames == 0 then sourceNames = {"无可用来源"} end
            values.selection:setValue(0)
            lastSources = sources
        end
        if processes ~= lastProcesses then
            processIds, processNames = {}, {}
            for pid in processes:gmatch("%d+") do
                processIds[#processIds+1] = pid; processNames[#processNames+1] = "SV2 · " .. pid
            end
            if #processNames == 0 then processNames = {"无可用实例"} end
            values.process:setValue(#processIds == 1 and 0 or -1)
            lastProcesses = processes
        end
        SV:refreshSidePanel()
    end
    local hasProcess = processIds[(tonumber(values.process:getValue()) or -1) + 1] ~= nil
    values.source:setEnabled(current.account and state.can_select_source == "1" and #sourceIds > 0 and hasProcess)
    values.main:setEnabled(current.account and state.can_restore_main == "1" and hasProcess)
    values.send:setEnabled(current.chat)
    values.status:setValue(current.live and (
        "Boxy: " .. (state.boxy_connected == "1" and "已连接" or "未连接") ..
        "  高级: " .. (state.boxy_authorized == "1" and "已授权" or "未授权") ..
        "  MCP: " .. (state.mcp_available == "1" and "在线" or "未启用") ..
        "  声梭 Connect: " .. (state.agent_available == "1" and "在线" or "未启用")
    ) or "Boxy 未连接：账号切换与聊天不可用")
    if current.live and state.boxy_authorized == "1" then
        local mode = decode(state.mode_hex)
        local phase = decode(state.phase_hex)
        local network = decode(state.network_hex)
        local modeText = mode == "main" and "主账号" or mode == "source" and "来源账号" or "账号状态未确认"
        local phaseText = (phase == "idle" or phase == "status") and "" or phase == "error" and " · 切换失败" or " · 切换中"
        local networkText = network == "blocked" and "SV2主程序联网已阻断" or network == "allowed" and "允许联网" or "网络未确认"
        values.status:setValue(values.status:getValue() .. "\n" .. modeText .. phaseText .. "\n" .. networkText)
    end
    local error = decode(state.error_hex)
    if error ~= "" then values.status:setValue(values.status:getValue() .. " · " .. error) end
    if state.response_id and state.response_id ~= lastResponse then
        lastResponse = state.response_id
        values.conversation:setValue(decode(state.conversation_hex))
    end
    SV:setTimeout(500, refresh)
end

function getClientInfo()
    return {name="Boxy Connect", author="Boxy", category="Boxy", versionNumber=1, minEditorVersion=131330, type="SidePanelSection"}
end
function getTranslations(_) return {} end
function getSidePanelSectionState()
    local function row(widget) return {type="Container", columns={widget}} end
    return {title="声梭 Connect", rows={
        row({type="TextArea", value=values.status, height=72, width=1.0}),
        row({type="ComboBox", choices=processNames, value=values.process, width=1.0}),
        row({type="ComboBox", choices=sourceNames, value=values.selection, width=1.0}),
        row({type="Button", text="切换来源账号", value=values.source, width=0.5}),
        row({type="Button", text="恢复主账号", value=values.main, width=0.5}),
        row({type="TextArea", value=values.conversation, height=180, width=1.0}),
        row({type="TextBox", value=values.input, width=1.0}),
        row({type="Button", text="发送", value=values.send, width=1.0})
    }}
end
refresh()
