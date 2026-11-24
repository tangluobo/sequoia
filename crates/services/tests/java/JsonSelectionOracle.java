import javafx.application.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.embed.swing.SwingFXUtils;
import org.fxmisc.richtext.InlineCssTextArea;
import javax.imageio.ImageIO;
import java.nio.file.*;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.JsonToolPane;
import com.tangluobo.tomato.module.tools.JsonFoldableTextView;

/** Owned original component selection probe; never writes the system clipboard. */
public class JsonSelectionOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class Probe extends Application {
  Parent pane;Stage stage;Path output;String mode;
  Object field(String name)throws Exception{var f=pane.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(pane);}
  Button button(Parent root,String title){for(Node n:root.getChildrenUnmodifiable()){if(n instanceof Button b&&b.getText().equals(title))return b;if(n instanceof Parent p){Button b=button(p,title);if(b!=null)return b;}}return null;}
  public void start(Stage stage)throws Exception{
   this.stage=stage;var args=getParameters().getRaw();mode=args.get(0);String source=Files.readString(Path.of(args.get(1)));output=Path.of(args.get(2));
   pane=mode.startsWith("mq")?new JsonFoldableTextView():new JsonToolPane();stage.initStyle(StageStyle.UNDECORATED);stage.setScene(new Scene(pane,1100,700));stage.setOpacity(.01);stage.show();stage.toBack();
   if(pane instanceof JsonFoldableTextView view){view.setText(source);if(mode.equals("mq-folded"))view.collapseAll();Platform.runLater(this::record);return;}
   Platform.runLater(()->{try{((TextArea)field("inputArea")).setText(source);button(pane,mode.equals("raw")?"去除转义":"格式化").fire();Platform.runLater(this::select);}catch(Exception e){e.printStackTrace();System.exit(1);}});
  }
  void select(){try{
   if(mode.equals("folded")){button(pane,"折叠全部").fire();Platform.runLater(this::record);return;}
   record();
  }catch(Exception e){e.printStackTrace();System.exit(1);}}
  void record(){try{
   String displayed,selected;
   if(mode.equals("raw")){TextArea area=(TextArea)field("rawOutputArea");area.requestFocus();area.selectAll();displayed=area.getText();selected=area.getSelectedText();}
   else{InlineCssTextArea area=(InlineCssTextArea)field("jsonRichArea");area.requestFocus();area.selectAll();displayed=area.getText();selected=area.getSelectedText();}
   if(!displayed.equals(selected))throw new IllegalStateException("Original full selection mismatch");
   Files.writeString(Path.of(output+"-visible.txt"),displayed);Files.writeString(Path.of(output+"-selected.txt"),selected);
   pane.applyCss();pane.layout();ImageIO.write(SwingFXUtils.fromFXImage(stage.getScene().snapshot(null),null),"png",Path.of(output+".png").toFile());
   JsonObject result=new JsonObject();result.addProperty("case",mode);result.addProperty("displayedUtf16Length",displayed.length());result.addProperty("selectedAllMatches",true);result.addProperty("systemClipboardWritten",false);result.addProperty("physicalOsInputUsed",false);JsonArray nodes=new JsonArray();ToolPaneOracle.nodes(pane,nodes);result.add("nodes",nodes);Files.writeString(Path.of(output+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(result));
   stage.hide();Platform.exit();
  }catch(Exception e){e.printStackTrace();System.exit(1);}}
 }
}
