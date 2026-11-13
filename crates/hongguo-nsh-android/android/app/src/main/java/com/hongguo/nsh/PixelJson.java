package com.hongguo.nsh;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

final class PixelJson {
    private PixelJson() {}
    static Map<String, Object> map(JSONObject object) throws JSONException {
        Map<String, Object> result = new HashMap<>();
        for (Iterator<String> keys = object.keys(); keys.hasNext();) {
            String key = keys.next();
            result.put(key, value(object.get(key)));
        }
        return result;
    }
    private static Object value(Object object) throws JSONException {
        if (object == JSONObject.NULL) return null;
        if (object instanceof JSONObject nested) return map(nested);
        if (object instanceof JSONArray array) {
            ArrayList<Object> result = new ArrayList<>();
            for (int index = 0; index < array.length(); index++) result.add(value(array.get(index)));
            return result;
        }
        return object;
    }
    static JSONObject event(String type, String sourceId, Object... fields) {
        try {
            JSONObject payload = new JSONObject().put("sourceId", sourceId);
            for (int index = 0; index < fields.length; index += 2) payload.put((String) fields[index], fields[index + 1]);
            return new JSONObject().put("type", type).put("payload", payload);
        } catch (JSONException error) { throw new IllegalArgumentException("Invalid native video event", error); }
    }
}
