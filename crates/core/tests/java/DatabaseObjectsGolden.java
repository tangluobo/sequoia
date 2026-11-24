import com.google.gson.*;
import com.tangluobo.tomato.module.connect.service.DatabaseService;
import java.nio.file.*;
import java.util.Locale;
/** Calls the original private formatter only: no database or saved configuration. */
public class DatabaseObjectsGolden {
 public static void main(String[] args)throws Exception {
  Locale.setDefault(Locale.US);
  var formatter=DatabaseService.class.getDeclaredMethod("formatDataLength",long.class);
  formatter.setAccessible(true);
  var rows=new JsonArray();
  long[] values={1,512,1023,1024,1088,1152,1280,1664,1048575,1048576,1179648,1073741823,1073741824,1207959552,1099511627775L,1099511627776L,1236950581248L,Long.MAX_VALUE};
  for(long bytes:values){var row=new JsonObject();row.addProperty("bytes",bytes);row.addProperty("display",(String)formatter.invoke(null,bytes));rows.add(row);}
  Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().create().toJson(rows));
 }
}
