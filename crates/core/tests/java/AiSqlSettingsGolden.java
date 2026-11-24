import com.google.gson.*;
import com.tangluobo.tomato.module.connect.GlobalConfig;
import java.nio.file.*;
import java.util.*;
/** Fresh in-memory configurations only. Never reads or saves global.json. */
public class AiSqlSettingsGolden {
 public static void main(String[] args)throws Exception{
  var gson=new GsonBuilder().serializeNulls().setPrettyPrinting().disableHtmlEscaping().create();var out=new JsonObject();var loads=new JsonArray();
  for(String json:List.of("{}","{\"aiApiBaseUrl\":null,\"aiModel\":null,\"aiApiKey\":null}","{\"aiApiBaseUrl\":\"  http://fixture.invalid/v1  \",\"aiModel\":\" fixture \",\"aiApiKey\":\" legacy key \",\"aiIncludeSchema\":false,\"aiRequestTimeoutSeconds\":999}","{\"aiModel\":\"\\u2003\",\"aiApiKey\":\" \",\"aiRequestTimeoutSeconds\":-2}","{\"aiApiKey\":\"TOMATO_ENCRYPTED:invalid\"}")){
   var config=gson.fromJson(json,GlobalConfig.class);var item=new JsonObject();item.add("input",JsonParser.parseString(json));item.add("output",read(config));loads.add(item);
  }out.add("loads",loads);var saves=new JsonArray();
  for(int timeout:new int[]{-1,0,1,5,60,300,301,999})for(String key:Arrays.asList(null,"","  fixture-secret-😀  ")){
   var config=new GlobalConfig();config.setAiApiBaseUrl(" http://fixture.invalid/v1 ");config.setAiModel(" fixture ");config.setAiApiKey(key);config.setAiIncludeSchema(false);config.setAiRequestTimeoutSeconds(timeout);var item=new JsonObject();item.addProperty("timeout",timeout);item.addProperty("key",key);item.add("output",read(config));item.addProperty("storedKey",gson.toJsonTree(config).getAsJsonObject().get("aiApiKey").getAsString());saves.add(item);
  }out.add("saves",saves);Files.writeString(Path.of(args[0]),gson.toJson(out));
 }
 static JsonObject read(GlobalConfig config){var out=new JsonObject();out.addProperty("base",config.getAiApiBaseUrl());out.addProperty("model",config.getAiModel());out.addProperty("key",config.getAiApiKey());out.addProperty("includeSchema",config.isAiIncludeSchema());out.addProperty("timeout",config.getAiRequestTimeoutSeconds());out.addProperty("configured",config.isAiSqlConfigured());return out;}
}
