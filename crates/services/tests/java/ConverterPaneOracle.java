import javafx.application.*;
import javafx.animation.*;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import java.util.*;
import javax.imageio.ImageIO;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.*;

/** Runs only the current original converter components against owned fixtures. */
public class ConverterPaneOracle {
 public static void main(String[] args) { Application.launch(Probe.class,args); }
 public static class Probe extends Application {
  Parent pane; Stage stage; Path capture; Button button; Label status;
  void snapshot(String suffix) throws Exception {
   pane.applyCss();pane.layout();
   ImageIO.write(SwingFXUtils.fromFXImage(stage.getScene().snapshot(null),null),"png",Path.of(capture+suffix+".png").toFile());
   JsonObject data=new JsonObject();data.addProperty("ownedOriginalComponent",true);data.addProperty("status",status.getText());data.addProperty("buttonDisabled",button.isDisabled());
   JsonArray nodes=new JsonArray();ToolPaneOracle.nodes(pane,nodes);data.add("nodes",nodes);
   Files.writeString(Path.of(capture+suffix+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(data));
  }
  void field(String name,String value) throws Exception {
   var member=pane.getClass().getDeclaredField(name);member.setAccessible(true);((TextField)member.get(pane)).setText(value);
  }
  public void start(Stage stage) throws Exception {
   this.stage=stage;var args=getParameters().getRaw();String kind=args.get(0);Path source=Path.of(args.get(1));Path destination=Path.of(args.get(2));capture=Path.of(args.get(3));
   pane=kind.equals("dataset")?new DatasetConverterPane():new ImageFormatConverterPane();
   var member=pane.getClass().getDeclaredField("convertButton");member.setAccessible(true);button=(Button)member.get(pane);
   member=pane.getClass().getDeclaredField("statusLabel");member.setAccessible(true);status=(Label)member.get(pane);
   stage.initStyle(StageStyle.UNDECORATED);stage.setScene(new Scene(pane,895,600));stage.setOpacity(.01);stage.show();stage.toBack();
   snapshot("-empty");
   field("sourcePathField",source.toString());field("targetPathField",destination.toString());
   if(!kind.equals("dataset")&&!Files.isDirectory(source)) {
    member=pane.getClass().getDeclaredField("isDirMode");member.setAccessible(true);member.set(pane,false);
    member=pane.getClass().getDeclaredField("svgFiles");member.setAccessible(true);member.set(pane,new ArrayList<>(List.of(source.toFile())));
   }
   button.fire();
   Timeline timer=new Timeline();
   timer.getKeyFrames().add(new KeyFrame(Duration.millis(100),e->{try {
    if(!button.isDisabled()) {
     snapshot("-completed");timer.stop();stage.hide();Platform.exit();
    }
   }catch(Exception error){error.printStackTrace();System.exit(1);}}));
   timer.setCycleCount(900);timer.play();
  }
 }
}
