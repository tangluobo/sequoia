import javafx.application.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.input.*;
import javafx.stage.*;
import org.fxmisc.richtext.InlineCssTextArea;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.JsonToolPane;
import java.nio.file.*;
public class JsonKeyboardOracle {
 public static void main(String[]args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  JsonObject results=new JsonObject();String text="hello world\nx\nlast";
  public void start(Stage stage)throws Exception{
   JsonToolPane pane=new JsonToolPane();stage.setScene(new Scene(pane,900,600));stage.setOpacity(.01);stage.show();stage.toBack();
   Platform.runLater(()->{try{
    var field=JsonToolPane.class.getDeclaredField("jsonRichArea");field.setAccessible(true);InlineCssTextArea rich=(InlineCssTextArea)field.get(pane);rich.replaceText(text);rich.requestFocus();
    JsonObject rr=new JsonObject();
    for(String mode:new String[]{"ctrlRight","ctrlLeft","downLast","upFirst","home","end"}){
      int at=switch(mode){case "ctrlRight"->2;case "ctrlLeft"->8;case "downLast"->15;case "upFirst"->2;case "home","end"->8;default->0;};
      rich.selectRange(at,at);KeyCode key=switch(mode){case "ctrlRight"->KeyCode.RIGHT;case "ctrlLeft"->KeyCode.LEFT;case "downLast"->KeyCode.DOWN;case "upFirst"->KeyCode.UP;case "home"->KeyCode.HOME;default->KeyCode.END;};
      rich.fireEvent(new KeyEvent(KeyEvent.KEY_PRESSED,"","",key,false,mode.startsWith("ctrl"),false,false));rr.addProperty(mode,rich.getCaretPosition());
    }results.add("rich",rr);
    var method=JsonToolPane.class.getDeclaredMethod("showRawView",String.class);method.setAccessible(true);method.invoke(pane,text);pane.applyCss();pane.layout();field=JsonToolPane.class.getDeclaredField("rawOutputArea");field.setAccessible(true);TextArea raw=(TextArea)field.get(pane);raw.requestFocus();JsonObject rp=new JsonObject();
    for(String mode:new String[]{"ctrlRight","ctrlLeft","downLast","upFirst","home","end"}){
      int at=switch(mode){case "ctrlRight"->2;case "ctrlLeft"->8;case "downLast"->15;case "upFirst"->2;case "home","end"->8;default->0;};raw.selectRange(at,at);
      KeyCode key=switch(mode){case "ctrlRight"->KeyCode.RIGHT;case "ctrlLeft"->KeyCode.LEFT;case "downLast"->KeyCode.DOWN;case "upFirst"->KeyCode.UP;case "home"->KeyCode.HOME;default->KeyCode.END;};raw.fireEvent(new KeyEvent(KeyEvent.KEY_PRESSED,"","",key,false,mode.startsWith("ctrl"),false,false));rp.addProperty(mode,raw.getCaretPosition());
    }results.add("raw",rp);results.addProperty("systemClipboardWritten",false);results.addProperty("physicalOsInputUsed",false);Files.writeString(Path.of(getParameters().getRaw().get(0)),new GsonBuilder().setPrettyPrinting().create().toJson(results));stage.hide();Platform.exit();
   }catch(Exception e){e.printStackTrace();System.exit(1);}});
  }
 }
}
