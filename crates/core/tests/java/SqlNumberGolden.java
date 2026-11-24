import com.google.gson.*;
import java.nio.file.*;
import java.math.BigDecimal;
import java.util.*;
/** Pure Java numeric string forms used by the original MySQL JDBC string factory. */
public class SqlNumberGolden {
 public static void main(String[] args)throws Exception{
  var result=new JsonObject();var decimals=new JsonArray();
  for(String input:new String[]{"0","0.0","0.000000","0.0000000","1","1.00","-1.000","0.000001","0.0000001","0.0000001000","-0.0000001000","0.0000000001","12345678901234567890.1234567890","99999999999999999999999999999999999999.0000000000000000000000000000"}){var item=new JsonObject();item.addProperty("input",input);item.addProperty("output",new BigDecimal(input).toString());decimals.add(item);}
  result.add("decimal",decimals);var floats=new JsonArray();
  for(String input:new String[]{"0","-0","1","-1","1.20","0.001","0.0001","1000000","10000000","1e100","1e-100","-123.456","1.7976931348623157e308","4.9406564584124654e-324","9.8813129168249309e-324","2.2250738585072014e-308","2.2250738585072012e-308","Infinity","-Infinity","NaN"})add(floats,input);
  var random=new Random(39041279L);for(int i=0;i<256;i++){double value=Double.longBitsToDouble(random.nextLong());if(Double.isFinite(value))add(floats,String.format(Locale.ROOT,"%.17e",value));}
  result.add("double",floats);Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().create().toJson(result));
 }
 static void add(JsonArray rows,String input){var item=new JsonObject();item.addProperty("input",input);item.addProperty("output",Double.toString(Double.parseDouble(input)));rows.add(item);}
}
