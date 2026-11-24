import com.google.gson.*;
import java.io.*;
import java.nio.charset.StandardCharsets;

public class JsonLenientOracle {
  public static void main(String[] args) throws Exception {
    System.setOut(new PrintStream(new FileOutputStream(FileDescriptor.out), true, StandardCharsets.UTF_8));
    Gson compact = new Gson();
    Gson pretty = new GsonBuilder().setPrettyPrinting().create();
    try (BufferedReader reader = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8))) {
      String line;
      while ((line = reader.readLine()) != null) {
        String input = JsonParser.parseString(line).getAsString();
        JsonObject result = new JsonObject();
        result.addProperty("input", input);
        try {
          JsonElement value = JsonParser.parseString(input.trim());
          result.addProperty("compact", compact.toJson(value));
          result.addProperty("pretty", pretty.toJson(value));
        } catch (Exception error) {
          result.addProperty("error", true);
        }
        System.out.println(compact.toJson(result));
      }
    }
  }
}
