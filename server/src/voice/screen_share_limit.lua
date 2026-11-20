-- Atomic screen share limit management.
-- KEYS[1] = screenshare:limit:{room counter}
-- Every change renews the key's expiry (a day): a room that is gone leaves
-- nothing behind for long.
-- ARGV[1] = max_shares
-- ARGV[2] = operation: "start" | "stop" | "stop_n" | "check"
-- ARGV[3] = count (only for "stop_n", number of slots to release)
-- Returns: {allowed (1/0), current_count}

local key = KEYS[1]
local max = tonumber(ARGV[1])
local op = ARGV[2]

if op == "check" then
    local count = tonumber(redis.call('GET', key) or '0')
    if count >= max then return {0, count} end
    return {1, count}
elseif op == "start" then
    local count = tonumber(redis.call('GET', key) or '0')
    if count >= max then return {0, count} end
    local new = redis.call('INCR', key)
    redis.call('EXPIRE', key, 86400)
    return {1, new}
elseif op == "stop" then
    local count = tonumber(redis.call('GET', key) or '0')
    if count > 0 then
        count = redis.call('DECR', key)
        redis.call('EXPIRE', key, 86400)
    end
    return {1, count}
elseif op == "stop_n" then
    local n = tonumber(ARGV[3]) or 1
    local count = tonumber(redis.call('GET', key) or '0')
    if count >= n then
        local new_count = redis.call('DECRBY', key, n)
        redis.call('EXPIRE', key, 86400)
        return {1, new_count}
    elseif count > 0 then
        redis.call('SET', key, '0', 'EX', 86400)
        return {1, 0}
    else
        return {1, 0}
    end
end

return {0, -1}
