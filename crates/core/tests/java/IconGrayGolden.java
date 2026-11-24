import javafx.application.*;
import javafx.scene.image.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.connect.ConnectModule;
import java.nio.file.*;
/** Original private stopped-container grayscale; only synthetic pixels. */
public class IconGrayGolden {
 public static void main(String[] args){Platform.startup(()->{try{
  int[] colors={0xff000000,0xffffffff,0xffff0000,0xff00ff00,0xff0000ff,0xff112233,0xffabcdef,0xff0988df,0xff1966ff,0xff015399,0xffc0ffee,0xff07c160,0xff555555,0xff010101,0xfffefefe,0x00000000};
  var input=new WritableImage(colors.length,1);for(int i=0;i<colors.length;i++)input.getPixelWriter().setArgb(i,0,colors[i]);
  var method=ConnectModule.class.getDeclaredMethod("createGrayImage",Image.class);method.setAccessible(true);var output=(Image)method.invoke(new ConnectModule(),input);
  var cases=new JsonArray();for(int i=0;i<colors.length;i++){var item=new JsonObject();item.addProperty("input",Integer.toUnsignedLong(colors[i]));item.addProperty("output",Integer.toUnsignedLong(output.getPixelReader().getArgb(i,0)));cases.add(item);}
  Files.createDirectories(Path.of(args[0]).getParent());Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().create().toJson(cases));Platform.exit();System.exit(0);
 }catch(Throwable error){error.printStackTrace();System.exit(1);}});}
}
