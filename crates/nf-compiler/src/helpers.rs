use std::sync::atomic::{AtomicU16, Ordering};

pub const ARRAY_FIND: &str = "array_find";
pub const ARRAY_REMOVE_ELEMENT: &str = "array_remove_element";
pub const ARRAY_CLEAR: &str = "array_clear";
pub const MAP_COUNT: &str = "map_count";
pub const SELECT_VALUE: &str = "select_value";
pub const WATCH_VALUES: &str = "watch_values";
pub const DEBUG_BREAK: &str = "debug_break";
pub const LISTENERS_OF: &str = "listeners_of";
pub const DEBUG_ENTER: &str = "debug_enter";
pub const INTERFACE_CALL: &str = "interface_call";
pub const TIMELINE_RUN: &str = "timeline_run";
pub const TIMELINE_VALUE: &str = "timeline_value";
pub const IMPLEMENTS_INTERFACE: &str = "implements_interface";
pub const DEBUG_LEAVE: &str = "debug_leave";
pub const DEBUGGER_PORT: u16 = 47615;
pub const CUSTOM_SETTING: &str = "custom_setting";

static CONFIGURED_DEBUGGER_PORT: AtomicU16 = AtomicU16::new(DEBUGGER_PORT);

pub fn set_debugger_port(port: u16) {
    CONFIGURED_DEBUGGER_PORT.store(port, Ordering::Relaxed);
}

pub fn debugger_port() -> u16 {
    CONFIGURED_DEBUGGER_PORT.load(Ordering::Relaxed)
}

pub fn render(name: &str) -> String {
    let source = source(name);
    if name == DEBUG_BREAK {
        source.replace(&format!("127.0.0.1:{DEBUGGER_PORT}"), &format!("127.0.0.1:{}", debugger_port()))
    } else {
        source.to_owned()
    }
}

pub const ALL: [&str; 15] = [
    CUSTOM_SETTING,
    TIMELINE_VALUE,
    TIMELINE_RUN,
    IMPLEMENTS_INTERFACE,
    INTERFACE_CALL,
    ARRAY_FIND,
    ARRAY_REMOVE_ELEMENT,
    ARRAY_CLEAR,
    MAP_COUNT,
    SELECT_VALUE,
    WATCH_VALUES,
    DEBUG_BREAK,
    DEBUG_ENTER,
    DEBUG_LEAVE,
    LISTENERS_OF,
];

const TIMELINE_VALUE_SOURCE: &str = "local function timeline_value(keys, time)
	local count = #keys
	if count == 0 then
		return 0
	end
	if time <= keys[1][1] then
		return keys[1][2]
	end
	for index = 2, count do
		local after = keys[index]
		if time <= after[1] then
			local before = keys[index - 1]
			local span = after[1] - before[1]
			if span <= 0 then
				return after[2]
			end
			return before[2] + (after[2] - before[2]) * (time - before[1]) / span
		end
	end
	return keys[count][2]
end";

const TIMELINE_RUN_SOURCE: &str = "local function timeline_run(holder, key, action, length, looping, update, finished)
	local timeline = holder[key]
	if not timeline then
		timeline = { position = 0, direction = 1 }
		holder[key] = timeline
	end
	if action == \"stop\" then
		if timeline.timer then
			Timer.ClearInterval(timeline.timer)
			timeline.timer = nil
		end
		return
	end
	if action == \"start\" then
		timeline.position = 0
		timeline.direction = 1
	elseif action == \"reverse\" then
		timeline.direction = -1
	else
		timeline.direction = 1
	end
	if timeline.timer then
		return
	end
	timeline.timer = Timer.SetInterval(function()
		local position = timeline.position + timeline.direction / 30
		local done = false
		if position >= length then
			if looping then
				position = position - length
			else
				position = length
				done = true
			end
		elseif position <= 0 then
			if looping then
				position = position + length
			else
				position = 0
				done = true
			end
		end
		timeline.position = position
		if update then
			update(position)
		end
		if done then
			Timer.ClearInterval(timeline.timer)
			timeline.timer = nil
			if finished then
				finished()
			end
		end
	end, 33)
end";

const LISTENERS_OF_SOURCE: &str = "local dispatcher_listeners = setmetatable({}, { __mode = \"k\" })\nlocal function listeners_of(instance, dispatcher)\n\tlocal own = dispatcher_listeners[instance]\n\tif not own then\n\t\town = {}\n\t\tdispatcher_listeners[instance] = own\n\tend\n\tlocal listeners = own[dispatcher]\n\tif not listeners then\n\t\tlisteners = {}\n\t\town[dispatcher] = listeners\n\tend\n\treturn listeners\nend";

const DEBUG_BREAK_SOURCE: &str = r#"local function debug_break(location, ...)
	local function quote(value)
		return "\"" .. (tostring(value):gsub("[%c\"\\]", function(character) return string.format("\\u%04x", character:byte()) end)) .. "\""
	end
	local values = table.pack(...)
	local parts = {}
	for index = 1, values.n, 2 do
		parts[#parts + 1] = "[" .. quote(values[index]) .. "," .. quote(values[index + 1]) .. "]"
	end
	local stack = NANOSFLOW_STACK or {}
	local frames = {}
	for index = 1, #stack do
		frames[index] = quote(stack[index])
	end
	local uri = "http://127.0.0.1:47615"
	local pause = HTTP.Request
	local payload = "{\"location\":" .. quote(location) .. ",\"side\":" .. quote(pause and "server" or "client") .. ",\"values\":[" .. table.concat(parts, ",") .. "],\"stack\":[" .. table.concat(frames, ",") .. "]}"
	if not pause then
		HTTP.RequestAsync(uri, "/hit", HTTPMethod.POST, payload, "application/json")
		return
	end
	local response = pause(uri, "/pause", HTTPMethod.POST, payload, "application/json")
	while response and response.Status == 202 do
		response = pause(uri, "/wait", HTTPMethod.POST, response.Data, "application/json")
	end
	local answer = response ~= nil and response.Status == 200 and response.Data or "continue"
	if answer == "into" then
		NANOSFLOW_STEP = math.huge
	elseif answer == "over" then
		NANOSFLOW_STEP = #stack
	elseif answer == "out" then
		NANOSFLOW_STEP = #stack - 1
	else
		NANOSFLOW_STEP = nil
	end
end"#;

pub fn source(name: &str) -> &'static str {
    match name {
        ARRAY_FIND => "local function array_find(array, element)\n\tfor index, value in ipairs(array) do\n\t\tif value == element then\n\t\t\treturn index\n\t\tend\n\tend\nend",
        ARRAY_REMOVE_ELEMENT => "local function array_remove_element(array, element)\n\tfor index = #array, 1, -1 do\n\t\tif array[index] == element then\n\t\t\ttable.remove(array, index)\n\t\tend\n\tend\nend",
        LISTENERS_OF => LISTENERS_OF_SOURCE,
        CUSTOM_SETTING => "local custom_settings\nlocal function custom_setting(key, fallback)\n\tif not custom_settings then\n\t\tcustom_settings = Server.GetCustomSettings() or {}\n\tend\n\tlocal value = custom_settings[key]\n\tif value == nil then\n\t\treturn fallback\n\tend\n\treturn value\nend",
        TIMELINE_RUN => TIMELINE_RUN_SOURCE,
        TIMELINE_VALUE => TIMELINE_VALUE_SOURCE,
        IMPLEMENTS_INTERFACE => "local function implements_interface(target, interface)\n\tlocal kind = type(target)\n\treturn (kind == \"table\" or kind == \"userdata\") and target[interface] == true\nend",
        INTERFACE_CALL => "local function interface_call(target, interface, method, ...)\n\tlocal kind = type(target)\n\tif (kind == \"table\" or kind == \"userdata\") and target[interface] == true then\n\t\treturn target[method](target, ...)\n\tend\nend",
        DEBUG_ENTER => "local function debug_enter(location)\n\tlocal stack = NANOSFLOW_STACK\n\tif not stack then\n\t\tstack = {}\n\t\tNANOSFLOW_STACK = stack\n\tend\n\tstack[#stack + 1] = location\nend",
        DEBUG_LEAVE => "local function debug_leave()\n\tlocal stack = NANOSFLOW_STACK\n\tif stack then\n\t\tstack[#stack] = nil\n\tend\nend",
        SELECT_VALUE => "local function select_value(condition, if_true, if_false)\n\tif condition then\n\t\treturn if_true\n\tend\n\treturn if_false\nend",
        WATCH_VALUES => "local function watch_values(...)\n\tlocal values = table.pack(...)\n\tlocal parts = {}\n\tfor index = 1, values.n, 2 do\n\t\tparts[#parts + 1] = tostring(values[index]) .. \" = \" .. tostring(values[index + 1])\n\tend\n\treturn table.concat(parts, \", \")\nend",
        DEBUG_BREAK => DEBUG_BREAK_SOURCE,
        ARRAY_CLEAR => "local function array_clear(array)\n\tfor index = #array, 1, -1 do\n\t\tarray[index] = nil\n\tend\nend",
        _ => "local function map_count(map)\n\tlocal count = 0\n\tfor _ in pairs(map) do\n\t\tcount = count + 1\n\tend\n\treturn count\nend",
    }
}
