local root = assert(os.getenv("BOXY_PANEL_DIR"))
local separator = package.config:sub(1, 1)
local function path(name) return root .. separator .. name end
local widgets = {}
local timer
SV = {}
function SV:getHostInfo() return {osType = separator == "\\" and "Windows" or "Linux"} end
function SV:create(name)
    assert(name == "WidgetValue")
    local widget = {enabled = true, value = ""}
    function widget:setEnabled(value) self.enabled = value end
    function widget:setValue(value) self.value = value end
    function widget:getValue() return self.value end
    function widget:setValueChangeCallback(callback) self.callback = callback end
    widgets[#widgets + 1] = widget
    return widget
end
function SV:setTimeout(_, callback) timer = callback end
function SV:refreshSidePanel() end
local function hex(value) return (value:gsub(".", function(byte) return string.format("%02x", string.byte(byte)) end)) end
local function write(name, content)
    local file = assert(io.open(path(name), "wb"))
    assert(file:write(content)); file:close()
end
local function read(name)
    local file = assert(io.open(path(name), "rb"))
    local result = file:read("*a"); file:close(); return result
end
dofile("connector/sidebar/BoxyConnect.lua")
assert(getClientInfo().type == "SidePanelSection")
assert(not getClientInfo().name:find("[^%z\1-\127]"))
assert(not widgets[6].enabled and not widgets[7].enabled and not widgets[8].enabled)
write("status.txt", "updated_at=" .. os.time() .. "\nboxy_authorized=1\naccount_available=1\ncan_select_source=1\ncan_restore_main=1\nagent_available=1\nchat_available=1\nmcp_available=0\nsources_hex=736f757263652d31094163636f756e74\nprocesses_hex=313233\n")
timer()
assert(widgets[6].enabled and widgets[7].enabled and widgets[8].enabled)
assert(widgets[1].value:find("MCP: 未启用", 1, true))
widgets[2]:setValue(0); widgets[3]:setValue(0); widgets[6].callback()
assert(read("request.txt"):find("kind=account.source.select\npayload_hex=31323309736f757263652d31", 1, true))
os.remove(path("request.txt"))
widgets[5]:setValue("hello"); widgets[8].callback()
assert(read("request.txt"):find("kind=chat.send\npayload_hex=68656c6c6f", 1, true))
os.remove(path("request.txt"))
write("status.txt", "updated_at=" .. os.time() .. "\nboxy_authorized=1\naccount_available=1\ncan_select_source=1\ncan_restore_main=1\nagent_available=1\nchat_available=1\nmcp_available=0\nsources_hex=" .. hex("source-1\tAccount") .. "\nprocesses_hex=" .. hex("123\n456") .. "\n")
timer()
assert(widgets[3].value == -1 and not widgets[6].enabled and not widgets[7].enabled)
widgets[3]:setValue(1); timer()
assert(widgets[6].enabled and widgets[7].enabled)
widgets[7].callback()
assert(read("request.txt"):find("kind=account.main.restore\npayload_hex=343536", 1, true))
os.remove(path("request.txt"))
write("status.txt", "updated_at=1\nboxy_authorized=1\naccount_available=1\ncan_select_source=1\ncan_restore_main=1\nagent_available=1\n")
timer()
assert(not widgets[6].enabled and not widgets[7].enabled and not widgets[8].enabled)
os.remove(path("status.txt"))
print("panel behavior passed")
